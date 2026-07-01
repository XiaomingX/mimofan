#!/usr/bin/env node

const { runMimofanTui } = require("../scripts/run");

runMimofanTui().catch((error) => {
  console.error("Failed to start mimofan-tui:", error.message);
  process.exit(1);
});
