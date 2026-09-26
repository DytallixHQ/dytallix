const winston = require("winston");
const axios = require("axios");
const { logFaucetEvent } = require("../utils/artifactLogger");

const logger = winston.createLogger({
  level: process.env.LOG_LEVEL || "info",
  format: winston.format.combine(
    winston.format.timestamp(),
    winston.format.json()
  ),
  transports: [new winston.transports.Console()],
});

class DualTokenFaucetController {
  constructor() {
    this.rpcEndpoint = process.env.RPC_ENDPOINT || "http://127.0.0.1:3030";
    this.chainId = process.env.CHAIN_ID || "dyt-local-1";
    this.faucetAddress = process.env.FAUCET_ADDRESS || "dyt1faucet";

    // Dual token system configuration
    this.tokenConfig = {
      DGT: {
        amount: process.env.DGT_FAUCET_AMOUNT || "10000000udgt",
        denom: "udgt",
        name: "Dytallix Governance Token",
        description: "For governance voting and protocol decisions",
        maxBalance: 50000000,
        supply: "Fixed (1B DGT)",
        votingPower: "1 DGT = 1 Vote",
        perRequest: "10 DGT",
      },
      DRT: {
        amount: process.env.DRT_FAUCET_AMOUNT || "100000000udrt",
        denom: "udrt",
        name: "Dytallix Reward Token",
        description: "For rewards, incentives, and transaction fees",
        maxBalance: 500000000,
        supply: "Inflationary (~6% annual)",
        utility: "Staking rewards, AI payments, transaction fees",
        perRequest: "100 DRT",
      },
    };
  }

  async sendTokens(req, res) {
    try {
      const { address, tokenType = "both" } = req.body;
      const clientIp = req.ip || req.connection.remoteAddress;

      logger.info("Dual token faucet request received", {
        address,
        tokenType,
        clientIp,
      });

      // Validate address format
      if (!this.isValidAddress(address)) {
        return res.status(400).json({
          error: "Invalid address format",
          message: "Address must be a valid Dytallix address starting with dyt or dytallix",
        });
      }

      // Determine amounts to send
      const udgt = tokenType === "DRT" ? 0 : parseInt(this.tokenConfig.DGT.amount);
      const udrt = tokenType === "DGT" ? 0 : parseInt(this.tokenConfig.DRT.amount);

      // Call blockchain dev/faucet endpoint
      const response = await axios.post(`${this.rpcEndpoint}/dev/faucet`, {
        address,
        udgt,
        udrt,
      });

      if (!response.data.success) {
        throw new Error("Blockchain faucet returned unsuccessful response");
      }

      logger.info("Blockchain faucet credited tokens", {
        address,
        credited: response.data.credited,
      });

      // Build response
      const transactions = [];
      const totalSent = {};

      if (udgt > 0) {
        transactions.push({
          token: "DGT",
          amount: `${udgt}udgt`,
          amountFormatted: `${udgt / 1000000} DGT`,
          txHash: this.generateTxHash(),
          denom: "udgt",
          purpose: "Governance voting and protocol decisions",
        });
        totalSent.DGT = `${udgt}udgt`;
      }

      if (udrt > 0) {
        transactions.push({
          token: "DRT",
          amount: `${udrt}udrt`,
          amountFormatted: `${udrt / 1000000} DRT`,
          txHash: this.generateTxHash(),
          denom: "udrt",
          purpose: "Rewards, incentives, and transaction fees",
        });
        totalSent.DRT = `${udrt}udrt`;
      }

      logFaucetEvent("SUCCESS_FUND", { address, tokenType, totalSent, transactions });

      res.status(200).json({
        success: true,
        message: `Successfully sent ${tokenType} tokens to ${address}`,
        recipient: address,
        tokenType,
        transactions,
        totalSent,
        blockchainResponse: response.data.credited,
        timestamp: new Date().toISOString(),
        note: this.getTokenTypeNote(tokenType),
      });
    } catch (error) {
      logger.error("Error in dual token distribution", {
        error: error.message,
        stack: error.stack,
      });

      res.status(500).json({
        error: "Blockchain error",
        message: error.message || "Failed to send tokens. Please try again later.",
      });
    }
  }

  getTokenTypeNote(tokenType) {
    switch (tokenType) {
      case "DGT":
        return "Received DGT tokens for governance voting.";
      case "DRT":
        return "Received DRT tokens for rewards and transactions.";
      case "both":
        return "Received both DGT (governance) and DRT (rewards) tokens.";
      default:
        return "Token distribution completed.";
    }
  }

  async getStatus(req, res) {
    try {
      // Check blockchain connection
      let networkStatus = { connected: false, error: null };
      try {
        const response = await axios.get(`${this.rpcEndpoint}/status`);
        networkStatus = {
          connected: true,
          chainId: response.data.chain_id,
          latestHeight: response.data.latest_height,
          syncing: response.data.syncing,
        };
      } catch (err) {
        networkStatus.error = err.message;
      }

      res.json({
        status: networkStatus.connected ? "healthy" : "degraded",
        faucetAddress: this.faucetAddress,
        chainId: this.chainId,
        network: networkStatus,
        tokenomics: {
          DGT: this.tokenConfig.DGT,
          DRT: this.tokenConfig.DRT,
        },
        supportedTokenTypes: ["DGT", "DRT", "both"],
        lastUpdated: new Date().toISOString(),
      });
    } catch (error) {
      res.status(500).json({ error: error.message });
    }
  }

  async getBalance(req, res) {
    try {
      const { address } = req.params;
      // Validate before interpolating into the RPC URL so a caller cannot inject
      // path segments (e.g. "../") and reach unintended backend endpoints.
      if (!this.isValidAddress(address)) {
        return res.status(400).json({ error: "Invalid address format" });
      }
      const response = await axios.get(
        `${this.rpcEndpoint}/balance/${encodeURIComponent(address)}`
      );
      res.json(response.data);
    } catch (error) {
      res.status(500).json({ error: error.message });
    }
  }

  async getAddressBalance(address, denom) {
    try {
      const response = await axios.get(`${this.rpcEndpoint}/balance/${address}`);
      const balances = response.data.balances || {};
      const bal = balances[denom];
      return bal?.balance ? parseInt(bal.balance) : 0;
    } catch {
      return 0;
    }
  }

  isValidAddress(address) {
    if (typeof address !== "string") {
      return false;
    }

    return /^(dyt1|dytallix1)[a-z0-9]{20,120}$/.test(address);
  }

  generateTxHash() {
    const chars = "0123456789ABCDEF";
    let hash = "";
    for (let i = 0; i < 64; i++) {
      hash += chars[Math.floor(Math.random() * 16)];
    }
    return hash;
  }
}

module.exports = new DualTokenFaucetController();
