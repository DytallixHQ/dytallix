const fs = require('fs');
const path = require('path');

// Resolve artifact log path relative to repository root
// __dirname -> faucet/src, go up two levels to repo root
const artifactDir = path.join(__dirname, '..', '..', 'launch-evidence', 'faucet');
const artifactLogPath = path.join(artifactDir, 'funding_and_rate_limit.log');

function ensureDir() {
  try {
    fs.mkdirSync(artifactDir, { recursive: true });
  } catch (_) {}
}

function logFaucetEvent(eventType, payload = {}) {
  try {
    ensureDir();
    const entry = {
      ts: new Date().toISOString(),
      type: eventType,
      ...payload,
    };
    fs.appendFileSync(artifactLogPath, JSON.stringify(entry) + '\n');
  } catch (err) {
    // Silent fail to avoid breaking API flow
  }
}

module.exports = {
  logFaucetEvent,
  artifactLogPath,
};
