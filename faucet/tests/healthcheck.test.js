const http = require('node:http');
const { checkHealth } = require('../src/healthcheck');

let server;
afterEach(async () => {
  if (server) {
    await new Promise((resolve) => server.close(resolve));
    server = undefined;
  }
});

async function serve(status, body) {
  server = http.createServer((_request, response) => {
    response.writeHead(status);
    response.end(body);
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  return server.address().port;
}

test('accepts the service health response', async () => {
  await expect(checkHealth(await serve(200, '{"status":"healthy"}'))).resolves.toBeUndefined();
});

test.each([
  [503, '{"status":"healthy"}'],
  [200, '{"status":"unhealthy"}'],
  [200, 'invalid JSON'],
])('rejects an invalid health response (%s, %s)', async (status, body) => {
  await expect(checkHealth(await serve(status, body))).rejects.toThrow();
});

test('fails when the service is unavailable', async () => {
  const port = await serve(200, '{"status":"healthy"}');
  await new Promise((resolve) => server.close(resolve));
  server = undefined;
  await expect(checkHealth(port)).rejects.toThrow();
});
