#!/bin/sh
# Capita Money – fetch the two face models of OpenCV Zoo and check them.
# SPDX-License-Identifier: AGPL-3.0-only
# YuNet: MIT licence (Shiqi Yu). SFace: Apache 2.0 (Yaoyao Zhong, Weihong Deng).
# Usage: ./fetch-models.sh <folder>   (default: models)
set -eu
dir=${1:-models}
mkdir -p "$dir"
base=https://media.githubusercontent.com/media/opencv/opencv_zoo/main/models
get() {
  file=$1; sum=$2
  if [ -f "$dir/$file" ] && echo "$sum  $dir/$file" | sha256sum -c --status; then return 0; fi
  curl -fsSL -o "$dir/$file" "$base/$3/$file"
  echo "$sum  $dir/$file" | sha256sum -c
}
get face_detection_yunet_2023mar.onnx 8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4 face_detection_yunet
get face_recognition_sface_2021dec.onnx 0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79 face_recognition_sface
