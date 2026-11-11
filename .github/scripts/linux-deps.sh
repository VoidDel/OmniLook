#!/usr/bin/env bash
# Installs the system libraries Tauri needs on the Ubuntu runners, plus any extra packages given
# as arguments. The runner's default sources (the Azure mirror list, archive.ubuntu.com and
# security.ubuntu.com) have failed to resolve for whole runs, with apt waiting on each mirror in
# turn for every package until the job timed out; so this points apt at the kernel.org mirror and
# makes it give up quickly instead.
set -euo pipefail

mirror=http://mirrors.edge.kernel.org/ubuntu/
sudo sed -i -E \
  "s#mirror\+file:/etc/apt/apt-mirrors\.txt|https?://(azure\.)?archive\.ubuntu\.com/ubuntu/?|https?://security\.ubuntu\.com/ubuntu/?#${mirror}#g" \
  /etc/apt/sources.list
grep -E '^deb ' /etc/apt/sources.list

apt=(sudo apt-get -o Acquire::Retries=3 -o Acquire::http::Timeout=20)
"${apt[@]}" update
"${apt[@]}" install -y \
  libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev "$@"
