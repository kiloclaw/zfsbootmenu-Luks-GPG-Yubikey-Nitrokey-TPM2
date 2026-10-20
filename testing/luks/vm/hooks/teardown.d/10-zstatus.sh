#!/bin/bash
# Test hook: report pool status and LUKS mappings before ZFSBootMenu tears down
echo "ZBMTEST-STATUS-BEGIN"
zpool status -P 2>&1 | sed 's/^/ZS| /'
zluks status 2>&1 | sed 's/^/ZL| /'
echo "ZBMTEST-STATUS-END"
