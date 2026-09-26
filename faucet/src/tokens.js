/**
 * Centralized Token Definitions for Dytallix Dual-Token System
 * Node.js/CommonJS version for faucet service
 */

const TOKENS = {
  DGT: {
    symbol: 'DGT',
    microDenom: 'udgt',
    decimals: 6,
    displayName: 'Dytallix Governance Token',
    role: 'governance',
    description: 'Used for governance voting, staking, fees, and protocol decisions'
  },
  DRT: {
    symbol: 'DRT', 
    microDenom: 'udrt',
    decimals: 6,
    displayName: 'Dytallix Reward Token',
    role: 'rewards',
    description: 'Used for rewards, incentives, staking rewards, and AI service payments'
  }
}

/**
 * Convert micro-denomination amount to display amount
 */
function formatAmount(amountInMicro, denom) {
  const token = Object.values(TOKENS).find(t => t.microDenom === denom)
  if (!token) {
    throw new Error(`Unknown denomination: ${denom}`)
  }
  
  const amount = typeof amountInMicro === 'string' ? parseFloat(amountInMicro) : amountInMicro
  const divisor = Math.pow(10, token.decimals)
  return (amount / divisor).toFixed(token.decimals)
}

/**
 * Convert display amount to micro-denomination amount
 */
function toMicroAmount(displayAmount, denom) {
  const token = Object.values(TOKENS).find(t => t.microDenom === denom)
  if (!token) {
    throw new Error(`Unknown denomination: ${denom}`)
  }
  
  const amount = typeof displayAmount === 'string' ? parseFloat(displayAmount) : displayAmount
  const multiplier = Math.pow(10, token.decimals)
  return Math.floor(amount * multiplier).toString()
}

/**
 * Get token metadata by micro denomination
 */
function getTokenByMicroDenom(microDenom) {
  const token = Object.values(TOKENS).find(t => t.microDenom === microDenom)
  if (!token) {
    throw new Error(`Unknown micro denomination: ${microDenom}`)
  }
  return token
}

/**
 * Get token metadata by symbol
 */
function getTokenBySymbol(symbol) {
  return TOKENS[symbol]
}

/**
 * Format amount with token symbol
 */
function formatAmountWithSymbol(amountInMicro, denom) {
  const token = getTokenByMicroDenom(denom)
  const formatted = formatAmount(amountInMicro, denom)
  return `${formatted} ${token.symbol}`
}

// Constants for common operations
const DGT_DECIMALS = TOKENS.DGT.decimals
const DRT_DECIMALS = TOKENS.DRT.decimals

const GOVERNANCE_TOKEN = TOKENS.DGT
const REWARD_TOKEN = TOKENS.DRT

module.exports = {
  TOKENS,
  formatAmount,
  toMicroAmount,
  getTokenByMicroDenom,
  getTokenBySymbol,
  formatAmountWithSymbol,
  DGT_DECIMALS,
  DRT_DECIMALS,
  GOVERNANCE_TOKEN,
  REWARD_TOKEN
}