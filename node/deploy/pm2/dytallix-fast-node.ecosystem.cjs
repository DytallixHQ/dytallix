module.exports = {
  apps: [
    {
      name: 'dytallix-fast-node',
      script: '/opt/dytallix-node/target/release/dytallix-fast-node',
      cwd: '/opt/dytallix-node',
      instances: 1,
      exec_mode: 'fork',
      watch: false,
      autorestart: true,
      max_memory_restart: '1G',
      env: {
        DYT_RPC_PORT: 3030,
      },
      env_file: '/etc/dytallix/dytallix-fast-node.env',
      error_file: '/var/log/dytallix/dytallix-fast-node-error.log',
      out_file: '/var/log/dytallix/dytallix-fast-node-out.log',
      merge_logs: true,
      time: true,
    },
  ],
};
