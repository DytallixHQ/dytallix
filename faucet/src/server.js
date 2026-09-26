const express = require('express');
const helmet = require('helmet');
const cors = require('cors');
const rateLimit = require('express-rate-limit');
const winston = require('winston');
const dotenv = require('dotenv');

// Load environment variables before reading feature flags.
dotenv.config();

const faucetController = require('./controllers/faucetController-dual');
const { errorHandler, notFound } = require('./middleware/errorHandler');
const { validateRequest } = require('./middleware/validation');
const rateLimitMiddleware = require('./middleware/rateLimit');

// Metrics setup
let metricsEnabled = false;
let metricsApp = null;
let prometheus = null;
let faucetMetrics = {};

if (process.env.ENABLE_METRICS === 'true') {
  metricsEnabled = true;
  prometheus = require('prom-client');
  
  // Create metrics registry
  const register = new prometheus.Registry();
  prometheus.collectDefaultMetrics({ register });
  
  // Faucet-specific metrics with dyt_ prefix
  faucetMetrics = {
    requestsTotal: new prometheus.Counter({
      name: 'dyt_faucet_requests_total',
      help: 'Total number of faucet requests',
      labelNames: ['status', 'token_type'],
      registers: [register]
    }),
    
    txLatency: new prometheus.Histogram({
      name: 'dyt_faucet_tx_latency_seconds',
      help: 'Faucet transaction processing latency',
      buckets: [0.1, 0.5, 1, 2, 5, 10, 30],
      registers: [register]
    }),
    
    apiRequestDuration: new prometheus.Histogram({
      name: 'dyt_api_request_duration_seconds',
      help: 'API request duration',
      labelNames: ['route', 'method', 'status'],
      buckets: [0.01, 0.05, 0.1, 0.5, 1, 5],
      registers: [register]
    })
  };
  
  // Create separate metrics server
  metricsApp = express();
  metricsApp.get('/metrics', async (req, res) => {
    try {
      res.set('Content-Type', register.contentType);
      res.end(await register.metrics());
    } catch (err) {
      res.status(500).end(err);
    }
  });
}

const app = express();

// Trust nginx proxy for correct client IP
app.set("trust proxy", 1);
const PORT = process.env.PORT || 3001;

// Configure Winston logger
const logger = winston.createLogger({
  level: process.env.LOG_LEVEL || 'info',
  format: winston.format.combine(
    winston.format.timestamp(),
    winston.format.json()
  ),
  transports: [
    new winston.transports.Console(),
    new winston.transports.File({ filename: process.env.LOG_FILE || 'logs/faucet.log' })
  ]
});

// Security middleware
app.use(helmet());
app.use(cors({
  origin: process.env.CORS_ORIGIN ? process.env.CORS_ORIGIN.split(',') : ['http://localhost:3000'],
  credentials: true
}));

// Body parsing middleware
app.use(express.json({ limit: '10mb' }));
app.use(express.urlencoded({ extended: true }));

// Global rate limiting
const globalLimiter = rateLimit({
  windowMs: 15 * 60 * 1000, // 15 minutes
  max: 100, // Limit each IP to 100 requests per windowMs
  message: {
    error: 'Too many requests from this IP, please try again later.',
    retryAfter: '15 minutes'
  },
  standardHeaders: true,
  legacyHeaders: false,
});
app.use(globalLimiter);

// Request logging and metrics middleware
app.use((req, res, next) => {
  const start = Date.now();
  
  logger.info(`${req.method} ${req.path}`, {
    ip: req.ip,
    userAgent: req.get('User-Agent'),
    timestamp: new Date().toISOString()
  });
  
  // Metrics collection
  if (metricsEnabled) {
    res.on('finish', () => {
      const duration = (Date.now() - start) / 1000;
      faucetMetrics.apiRequestDuration
        .labels(req.route?.path || req.path, req.method, res.statusCode.toString())
        .observe(duration);
    });
  }
  
  next();
});

// Health check endpoint
app.get('/health', (req, res) => {
  res.status(200).json({
    status: 'healthy',
    timestamp: new Date().toISOString(),
    service: 'dytallix-faucet',
    version: '1.0.0'
  });
});

// API info endpoint
app.get('/api/info', (req, res) => {
  res.json({
    service: 'Dytallix Testnet Faucet - Dual Token System',
    version: '2.0.0',
    chainId: process.env.CHAIN_ID || 'dyt-local-1',
    tokenSystem: 'Dual Token (DGT + DRT)',
    tokens: {
      DGT: {
        amount: process.env.DGT_FAUCET_AMOUNT || '10000000udgt',
        formatted: '10 DGT per request',
        purpose: 'Governance and voting',
        supply: 'Fixed (1B DGT)'
      },
      DRT: {
        amount: process.env.DRT_FAUCET_AMOUNT || '100000000udrt', 
        formatted: '100 DRT per request',
        purpose: 'Rewards and transaction fees',
        supply: 'Inflationary (~6% annual)'
      }
    },
    rateLimits: {
      windowMs: parseInt(process.env.RATE_LIMIT_WINDOW_MS) || 3600000,
      maxRequests: parseInt(process.env.RATE_LIMIT_MAX_REQUESTS) || 20,
      cooldownMs: parseInt(process.env.IP_COOLDOWN_MS) || 60000
    },
    endpoints: {
      faucet: '/api/faucet (supports tokenType: DGT, DRT, or both)',
      status: '/api/status',
      balance: '/api/balance/:address'
    },
    tokenomics: {
      DGT: 'Governance token for voting and protocol decisions',
      DRT: 'Reward token for staking rewards and transaction fees'
    }
  });
});

// Faucet endpoints with rate limiting
app.post('/api/faucet', 
  rateLimitMiddleware,
  validateRequest,
  (req, res) => faucetController.sendTokens(req, res)
);

app.get('/api/status', (req, res) => faucetController.getStatus(req, res));
app.get('/api/balance/:address', (req, res) => faucetController.getBalance(req, res));

// Alias for wallet compatibility - request endpoint
app.post("/api/faucet/request", 
  rateLimitMiddleware,
  validateRequest,
  (req, res) => {
    if (req.body.dgt_amount || req.body.drt_amount) {
      req.body.tokenType = "both";
    }
    return faucetController.sendTokens(req, res);
  }
);

// Error handling middleware
app.use(notFound);
app.use(errorHandler);

let mainServer;
let metricsServer;

function startServers() {
  if (!mainServer) {
    mainServer = app.listen(PORT, () => {
      logger.info(`Dytallix Faucet API server running on port ${PORT}`, {
        port: PORT,
        environment: process.env.NODE_ENV || 'development',
        chainId: process.env.CHAIN_ID || 'dyt-local-1'
      });
    });
  }

  if (metricsEnabled && metricsApp && !metricsServer) {
    const metricsPort = process.env.METRICS_PORT || 9101;
    metricsServer = metricsApp.listen(metricsPort, () => {
      logger.info(`Faucet metrics server running on port ${metricsPort}`);
    });
  }

  return { mainServer, metricsServer };
}

module.exports = app;
module.exports.startServers = startServers;
if (metricsEnabled) {
  module.exports.metrics = faucetMetrics;
}

if (require.main === module) {
  startServers();
}
