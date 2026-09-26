FROM node:24.20.0-alpine@sha256:e67514e5d0f6c46656005e1b693b2ec9d52e80b641307de684d4a015ba7a4eaf

# Set working directory
WORKDIR /app

# Create non-root user for security
RUN addgroup -g 1001 -S nodejs && \
    adduser -S faucet -u 1001

# Copy package files
COPY package*.json ./

# Install dependencies
RUN npm ci --omit=dev && \
    npm cache clean --force

# Copy application code
COPY src/ ./src/

# Create logs directory
RUN mkdir -p logs && \
    chown -R faucet:nodejs /app

# Switch to non-root user
USER faucet

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD npm run health

# Expose port
EXPOSE 3001

# Start the application
CMD ["npm", "start"]
