#!/usr/bin/env bash

set -eu

if [ ! -d irctest ]; then
    git clone https://github.com/progval/irctest
    cd irctest && git reset --hard d202e440bb71139e7c10e25bd04660866371c686 && cd ..
fi

uv pip install -r irctest/requirements.txt

cargo build --bin irctest-compat

PYTHONPATH=tests/testsuite-irctest/ pytest -v --controller cirque irctest/irctest/server_tests/pingpong.py -k 'not Ergo and not IRCv3'
