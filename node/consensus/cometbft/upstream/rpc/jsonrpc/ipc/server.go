package ipc

import (
	"context"
	"errors"
	"net"
	"os"
	"path/filepath"
	"sync"
	"time"
)

type Server struct {
	net.Listener
	mu          sync.Mutex
	closed      bool
	connections map[net.Conn]struct{}
	permits     chan struct{}
	handler     Handler
}

// Listen binds one private Unix socket. It never removes an existing path.
func Listen(path string, handler Handler) (*Server, error) {
	if handler == nil || !filepath.IsAbs(path) || filepath.Clean(path) != path || filepath.Base(path) != "rpc.sock" {
		return nil, errors.New("clean absolute rpc.sock path and handler required")
	}
	parent, err := os.Lstat(filepath.Dir(path))
	if err != nil {
		return nil, err
	}
	if !parent.IsDir() || parent.Mode()&os.ModeSymlink != 0 || parent.Mode().Perm() != 0700 {
		return nil, errors.New("RPC socket parent must be a real private directory with mode 0700")
	}
	if _, err = os.Lstat(path); err == nil || !os.IsNotExist(err) {
		return nil, errors.New("RPC socket path already exists or cannot be checked")
	}
	listener, err := net.ListenUnix("unix", &net.UnixAddr{Name: path, Net: "unix"})
	if err != nil {
		return nil, err
	}
	if err = os.Chmod(path, 0600); err != nil {
		_ = listener.Close()
		return nil, err
	}
	server := &Server{Listener: listener, connections: make(map[net.Conn]struct{}), permits: make(chan struct{}, MaxConnections), handler: handler}
	go server.serve()
	return server, nil
}
func (s *Server) serve() {
	for {
		conn, err := s.Listener.Accept()
		if err != nil {
			return
		}
		select {
		case s.permits <- struct{}{}:
		default:
			_ = conn.Close()
			continue
		}
		s.mu.Lock()
		if s.closed {
			s.mu.Unlock()
			_ = conn.Close()
			<-s.permits
			return
		}
		s.connections[conn] = struct{}{}
		s.mu.Unlock()
		go s.handle(conn)
	}
}
func (s *Server) handle(conn net.Conn) {
	defer func() {
		// A failed local handler must release its connection and worker permit.
		_ = recover()
		_ = conn.Close()
		s.mu.Lock()
		delete(s.connections, conn)
		s.mu.Unlock()
		<-s.permits
	}()
	deadline := time.Now().Add(RequestTimeout)
	_ = conn.SetDeadline(deadline)
	ctx, cancel := context.WithDeadline(context.Background(), deadline)
	defer cancel()
	stopClose := context.AfterFunc(ctx, func() { _ = conn.Close() })
	defer stopClose()
	frame, err := readFrame(conn)
	if err != nil {
		return
	}
	request, body, err := decodeRequest(frame)
	if err != nil {
		response, _ := marshalResponse(errorResponse(400, "invalid RPC IPC request"))
		_ = writeFrame(conn, response)
		return
	}
	// The protocol permits exactly one request. EOF, extra bytes or peer closure cancel the call.
	go func() { var extra [1]byte; _, _ = conn.Read(extra[:]); cancel() }()
	response := s.handler(ctx, request, body)
	raw, err := marshalResponse(response)
	if err != nil {
		raw, _ = marshalResponse(errorResponse(507, "RPC response exceeds experimental profile limit"))
	}
	_ = writeFrame(conn, raw)
}
func (s *Server) Close() error {
	s.mu.Lock()
	if s.closed {
		s.mu.Unlock()
		return nil
	}
	s.closed = true
	err := s.Listener.Close()
	for connection := range s.connections {
		_ = connection.Close()
	}
	s.mu.Unlock()
	return err
}
