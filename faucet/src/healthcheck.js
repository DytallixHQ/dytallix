const http = require('node:http');

function checkHealth(port = process.env.PORT || 3001, timeoutMs = 2000) {
  return new Promise((resolve, reject) => {
    const request = http.get({ host: '127.0.0.1', port, path: '/health' }, (response) => {
      let body = '';
      response.setEncoding('utf8');
      response.on('data', (chunk) => { body += chunk; });
      response.on('error', reject);
      response.on('end', () => {
        try {
          if (response.statusCode !== 200 || JSON.parse(body).status !== 'healthy') {
            throw new Error('Faucet health check failed');
          }
          resolve();
        } catch (error) {
          reject(error);
        }
      });
    });
    request.setTimeout(timeoutMs, () => request.destroy(new Error('Health check timed out')));
    request.on('error', reject);
  });
}

if (require.main === module) {
  checkHealth().catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}

module.exports = { checkHealth };
