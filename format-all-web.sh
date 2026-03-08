#!/bin/bash

pushd $(dirname $0) > /dev/null

pushd web && npm run prettier:write && popd > /dev/null
pushd api && npm run prettier:write && popd > /dev/null
pushd landing && npm run prettier:write && popd > /dev/null
pushd shared-web && npm run prettier:write && popd > /dev/null
pushd help && npm run prettier:write && popd > /dev/null

popd > /dev/null

