const winston = require('winston');

const logger = winston.createLogger({
  level: process.env.LOG_LEVEL || 'info',
  format: winston.format.combine(
    winston.format.timestamp(),
    winston.format.json()
  ),
  transports: [new winston.transports.Console()]
});

const errorHandler = (err, req, res, next) => {
  const clientIp = req.ip || req.connection.remoteAddress;
  
  logger.error('Unhandled error', {
    error: err.message,
    stack: err.stack,
    ip: clientIp,
    method: req.method,
    path: req.path,
    body: req.body,
    timestamp: new Date().toISOString()
  });

  // Don't leak error details in production
  const isDevelopment = process.env.NODE_ENV === 'development';
  
  res.status(err.status || 500).json({
    error: 'Internal server error',
    message: isDevelopment ? err.message : 'An unexpected error occurred',
    ...(isDevelopment && { stack: err.stack }),
    timestamp: new Date().toISOString()
  });
};

const notFound = (req, res, next) => {
  const clientIp = req.ip || req.connection.remoteAddress;
  
  logger.warn('404 - Route not found', {
    ip: clientIp,
    method: req.method,
    path: req.path,
    timestamp: new Date().toISOString()
  });

  res.status(404).json({
    error: 'Not found',
    message: `Route ${req.method} ${req.path} not found`,
    availableEndpoints: {
      faucet: 'POST /api/faucet',
      status: 'GET /api/status',
      balance: 'GET /api/balance/:address',
      info: 'GET /api/info',
      health: 'GET /health'
    },
    timestamp: new Date().toISOString()
  });
};

module.exports = {
  errorHandler,
  notFound
};