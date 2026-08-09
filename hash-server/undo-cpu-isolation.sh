#!/bin/bash
# Undo the cgroup v2 "bench" isolated cpuset partition (cores 6-7) and the
# LXD "first" container stop, both set up for hash-server benchmarking.
#
# Usage: ./undo-cpu-isolation.sh
set -e

echo "Removing isolated cpuset partitions..."
if [ -d /sys/fs/cgroup/bench ]; then
    # Move nested partitions back to "member" before removing (children first).
    for child in server client; do
        if [ -d "/sys/fs/cgroup/bench/$child" ]; then
            sudo bash -c "echo member > /sys/fs/cgroup/bench/$child/cpuset.cpus.partition" 2>/dev/null || true
            sudo rmdir "/sys/fs/cgroup/bench/$child"
            echo "  removed /sys/fs/cgroup/bench/$child"
        fi
    done
    sudo bash -c 'echo member > /sys/fs/cgroup/bench/cpuset.cpus.partition' 2>/dev/null || true
    sudo rmdir /sys/fs/cgroup/bench
    echo "  removed /sys/fs/cgroup/bench"
else
    echo "  /sys/fs/cgroup/bench does not exist, nothing to do"
fi

echo "Verifying root cpuset.cpus.effective is back to full range:"
cat /sys/fs/cgroup/cpuset.cpus.effective

echo
echo "Restarting the LXD container 'first' (it was stopped to free cores 6-7)..."
sudo lxc start first
sudo lxc list

echo
echo "Done. If you also want to disable the cpuset controller delegation at"
echo "root (it was already enabled before this script touched anything, so"
echo "this script leaves cgroup.subtree_control alone)."
