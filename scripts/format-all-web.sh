#!/bin/bash

pushd "$(dirname $0)/.." > /dev/null

pushd web && npm run format && popd > /dev/null
pushd api && npm run format && popd > /dev/null
pushd landing && npm run format && popd > /dev/null
pushd shared-web && npm run format && popd > /dev/null

popd > /dev/null
