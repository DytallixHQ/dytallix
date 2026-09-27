// Package metricsfile records engine metrics in memory and writes them as a
// Prometheus text-format file (Dytallix metrics v1). The PQC-only build links
// no metrics library with a listener: CometBFT's metric structs take go-kit
// interfaces, which this registry implements, and an operator agent such as
// the node_exporter textfile collector reads the file.
package metricsfile

import (
	"errors"
	"fmt"
	"io"
	"math"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/go-kit/kit/metrics"
)

// WrittenMetric carries each file's write time, so a stopped writer shows
// as stale.
const WrittenMetric = "dytallix_metrics_written_timestamp_seconds"

type kind int

const (
	counterKind kind = iota
	gaugeKind
	// Histograms are written as summaries: a sum and a count.
	summaryKind
)

func (k kind) String() string {
	return [...]string{"counter", "gauge", "summary"}[k]
}

type series struct {
	value float64
	sum   float64
	count uint64
}

type family struct {
	kind   kind
	series map[string]*series
}

// Registry holds every recorded series. It is safe for concurrent use.
type Registry struct {
	mu       sync.Mutex
	families map[string]*family
}

func NewRegistry() *Registry {
	return &Registry{families: map[string]*family{}}
}

// labels renders alternating name and value pairs, escaped, in the given
// order; an odd trailing name gets an empty value, as go-kit does.
func labels(pairs []string) string {
	if len(pairs) == 0 {
		return ""
	}
	var b strings.Builder
	b.WriteByte('{')
	for i := 0; i < len(pairs); i += 2 {
		if i > 0 {
			b.WriteByte(',')
		}
		value := ""
		if i+1 < len(pairs) {
			value = pairs[i+1]
		}
		replacer := strings.NewReplacer(`\`, `\\`, `"`, `\"`, "\n", `\n`)
		fmt.Fprintf(&b, `%s="%s"`, pairs[i], replacer.Replace(value))
	}
	b.WriteByte('}')
	return b.String()
}

func (r *Registry) update(name string, k kind, pairs []string, apply func(*series)) {
	r.mu.Lock()
	defer r.mu.Unlock()
	f, ok := r.families[name]
	if !ok {
		f = &family{kind: k, series: map[string]*series{}}
		r.families[name] = f
	}
	key := labels(pairs)
	s, ok := f.series[key]
	if !ok {
		s = &series{}
		f.series[key] = s
	}
	apply(s)
}

type counter struct {
	r     *Registry
	name  string
	pairs []string
}

func (c *counter) With(labelValues ...string) metrics.Counter {
	return &counter{r: c.r, name: c.name, pairs: append(append([]string{}, c.pairs...), labelValues...)}
}
func (c *counter) Add(delta float64) {
	c.r.update(c.name, counterKind, c.pairs, func(s *series) { s.value += delta })
}

type gauge struct {
	r     *Registry
	name  string
	pairs []string
}

func (g *gauge) With(labelValues ...string) metrics.Gauge {
	return &gauge{r: g.r, name: g.name, pairs: append(append([]string{}, g.pairs...), labelValues...)}
}
func (g *gauge) Set(value float64) {
	g.r.update(g.name, gaugeKind, g.pairs, func(s *series) { s.value = value })
}
func (g *gauge) Add(delta float64) {
	g.r.update(g.name, gaugeKind, g.pairs, func(s *series) { s.value += delta })
}

type histogram struct {
	r     *Registry
	name  string
	pairs []string
}

func (h *histogram) With(labelValues ...string) metrics.Histogram {
	return &histogram{r: h.r, name: h.name, pairs: append(append([]string{}, h.pairs...), labelValues...)}
}
func (h *histogram) Observe(value float64) {
	h.r.update(h.name, summaryKind, h.pairs, func(s *series) {
		s.sum += value
		s.count++
	})
}

func (r *Registry) Counter(name string) metrics.Counter { return &counter{r: r, name: name} }
func (r *Registry) Gauge(name string) metrics.Gauge     { return &gauge{r: r, name: name} }
func (r *Registry) Histogram(name string) metrics.Histogram {
	return &histogram{r: r, name: name}
}

func number(v float64) string {
	if math.IsInf(v, 1) {
		return "+Inf"
	}
	if math.IsInf(v, -1) {
		return "-Inf"
	}
	if math.IsNaN(v) {
		return "NaN"
	}
	return strconv.FormatFloat(v, 'g', -1, 64)
}

// Write renders every family, sorted by name and labels, then the write
// time with the given process label.
func (r *Registry) Write(w io.Writer, process string, now time.Time) error {
	r.mu.Lock()
	names := make([]string, 0, len(r.families))
	for name := range r.families {
		names = append(names, name)
	}
	sort.Strings(names)
	var b strings.Builder
	for _, name := range names {
		f := r.families[name]
		keys := make([]string, 0, len(f.series))
		for key := range f.series {
			keys = append(keys, key)
		}
		sort.Strings(keys)
		fmt.Fprintf(&b, "# TYPE %s %s\n", name, f.kind)
		for _, key := range keys {
			s := f.series[key]
			if f.kind == summaryKind {
				fmt.Fprintf(&b, "%s_sum%s %s\n", name, key, number(s.sum))
				fmt.Fprintf(&b, "%s_count%s %d\n", name, key, s.count)
			} else {
				fmt.Fprintf(&b, "%s%s %s\n", name, key, number(s.value))
			}
		}
	}
	r.mu.Unlock()
	fmt.Fprintf(&b, "# TYPE %s gauge\n%s%s %s\n", WrittenMetric, WrittenMetric,
		labels([]string{"process", process}), number(float64(now.UnixMilli())/1000))
	_, err := io.WriteString(w, b.String())
	return err
}

// WriteFile replaces dir/name with the registry's text, through a
// temporary file in dir and a rename.
func (r *Registry) WriteFile(dir, name, process string, now time.Time) error {
	if !filepath.IsAbs(dir) || filepath.Base(name) != name || !strings.HasSuffix(name, ".prom") {
		return errors.New("metrics file needs an absolute directory and a .prom name")
	}
	file, err := os.CreateTemp(dir, "."+name+"-")
	if err != nil {
		return err
	}
	defer os.Remove(file.Name())
	if err = r.Write(file, process, now); err == nil {
		err = file.Chmod(0o644)
	}
	if closeErr := file.Close(); err == nil {
		err = closeErr
	}
	if err != nil {
		return err
	}
	return os.Rename(file.Name(), filepath.Join(dir, name))
}
