const rateLimit = require('express-rate-limit');
const winston = require('winston');
const { logFaucetEvent } = require('../utils/artifactLogger');

const logger = winston.createLogger({
  level: process.env.LOG_LEVEL || 'info',
  format: winston.format.combine(
    winston.format.timestamp(),
    winston.format.json()
  ),
  transports: [new winston.transports.Console()]
});

// In-memory store for IP cooldowns (in production, use Redis)
const ipCooldowns = new Map();

const faucetRateLimit = rateLimit({
  windowMs: parseInt(process.env.RATE_LIMIT_WINDOW_MS) || 3600000, // 1 hour
  max: parseInt(process.env.RATE_LIMIT_MAX_REQUESTS) || 20, // 20 requests per hour
  standardHeaders: true,
  legacyHeaders: false,
  keyGenerator: (req) => {
    return req.ip || req.connection.remoteAddress;
  },
  message: {
    error: 'Rate limit exceeded',
    message: 'Too many faucet requests from this IP. Please wait before trying again.',
    retryAfter: Math.ceil((parseInt(process.env.RATE_LIMIT_WINDOW_MS) || 3600000) / 1000)
  },
  handler: (req, res, next, options) => {
    const clientIp = req.ip || req.connection.remoteAddress;
    logger.warn('Rate limit exceeded', {
      ip: clientIp,
      userAgent: req.get('User-Agent'),
      timestamp: new Date().toISOString()
    });
    // Artifact evidence logging (rate-limit enforcement)
    logFaucetEvent('RATE_LIMIT', {
      ip: clientIp,
      route: req.originalUrl,
      method: req.method,
      userAgent: req.get('User-Agent'),
      retryAfter: Math.ceil((parseInt(process.env.RATE_LIMIT_WINDOW_MS) || 3600000) / 1000),
      status: options.statusCode,
    });
    
    res.status(options.statusCode).json(options.message);
  }
});

// Additional IP cooldown middleware
const ipCooldownMiddleware = (req, res, next) => {
  const clientIp = req.ip || req.connection.remoteAddress;
  const cooldownMs = parseInt(process.env.IP_COOLDOWN_MS) || 60000; // 60 seconds
  const now = Date.now();

  if (ipCooldowns.has(clientIp)) {
    const lastRequest = ipCooldowns.get(clientIp);
    const timeSinceLastRequest = now - lastRequest;

    if (timeSinceLastRequest < cooldownMs) {
      const remainingTime = Math.ceil((cooldownMs - timeSinceLastRequest) / 1000);
      
      logger.warn('IP cooldown active', {
        ip: clientIp,
        remainingSeconds: remainingTime,
        timestamp: new Date().toISOString()
      });

      // Artifact evidence logging (cooldown enforcement)
      logFaucetEvent('IP_COOLDOWN', {
        ip: clientIp,
        route: req.originalUrl,
        method: req.method,
        remainingSeconds: remainingTime,
        cooldownMs,
        status: 429,
      });

      return res.status(429).json({
        error: 'IP cooldown active',
        message: `Please wait ${remainingTime} seconds before making another faucet request`,
        retryAfter: remainingTime,
        cooldownMs: cooldownMs
      });
    }
  }

  // Update last request time
  ipCooldowns.set(clientIp, now);

  // Clean up old entries (older than 2 hours)
  const cutoff = now - (2 * 60 * 60 * 1000);
  for (const [ip, timestamp] of ipCooldowns.entries()) {
    if (timestamp < cutoff) {
      ipCooldowns.delete(ip);
    }
  }

  next();
};

// Combine rate limiting middleware
const rateLimitMiddleware = [faucetRateLimit, ipCooldownMiddleware];

module.exports = rateLimitMiddleware;
