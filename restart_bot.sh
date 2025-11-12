#!/bin/bash
# Restart the market maker bot
# This script gracefully stops the bot and restarts it

echo "Stopping market maker bot..."
bash kill_process.sh

echo "Waiting for graceful shutdown (5 seconds)..."
sleep 2

echo "Starting market maker bot..."
bash run_nohup.sh

echo "Bot restarted. Check output.log for status:"
sleep 2
tail -n 20 output.log
