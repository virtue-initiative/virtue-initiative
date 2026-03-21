#!/bin/bash

pushd $(dirname $0) > /dev/null

pushd ./api > /dev/null && npm run deploy
popd && pushd ./web > /dev/null && npm run deploy
popd && pushd ./landing > /dev/null && npm run deploy

popd
