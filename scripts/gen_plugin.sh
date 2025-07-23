#!/bin/bash

cd ..
satcnt=true
rdwr=true
rot=true
for pht in 16 32 64 128 256 512 1024 perfect
do
    srcfile="$(pwd)/src/configs/parameter_${satcnt}_${rdwr}_${rot}_${pht}.rs"
    dstfile="$(pwd)/src/parameter.rs"
    echo "Copying ${srcfile} to ${dstfile}"
    cp $srcfile $dstfile

    echo "Building ..."
    cargo build --release --bin worm_cache

    srcfile="$(pwd)/target/release/worm_cache"
    dstfile="$(pwd)/../bins/pf/worm_cache_${satcnt}_${rdwr}_${rot}_${pht}"
    cp $srcfile $dstfile
done

satcnt=true
rdwr=true
rot=false
for pht in 16 32 64 128 256 512 1024 perfect
do
    srcfile="$(pwd)/src/configs/parameter_${satcnt}_${rdwr}_${rot}_${pht}.rs"
    dstfile="$(pwd)/src/parameter.rs"
    echo "Copying ${srcfile} to ${dstfile}"
    cp $srcfile $dstfile

    echo "Building ..."
    cargo build --release --bin worm_cache

    srcfile="$(pwd)/target/release/worm_cache"
    dstfile="$(pwd)/../bins/pf/worm_cache_${satcnt}_${rdwr}_${rot}_${pht}"
    cp $srcfile $dstfile
done

satcnt=true
rdwr=false
rot=false
for pht in 16 32 64 128 256 512 1024 perfect
do
    srcfile="$(pwd)/src/configs/parameter_${satcnt}_${rdwr}_${rot}_${pht}.rs"
    dstfile="$(pwd)/src/parameter.rs"
    echo "Copying ${srcfile} to ${dstfile}"
    cp $srcfile $dstfile

    echo "Building ..."
    cargo build --release --bin worm_cache

    srcfile="$(pwd)/target/release/worm_cache"
    dstfile="$(pwd)/../bins/pf/worm_cache_${satcnt}_${rdwr}_${rot}_${pht}"
    cp $srcfile $dstfile
done

satcnt=false
rdwr=false
rot=false
for pht in 16 32 64 128 256 512 1024 perfect
do
    srcfile="$(pwd)/src/configs/parameter_${satcnt}_${rdwr}_${rot}_${pht}.rs"
    dstfile="$(pwd)/src/parameter.rs"
    echo "Copying ${srcfile} to ${dstfile}"
    cp $srcfile $dstfile

    echo "Building ..."
    cargo build --release --bin worm_cache

    srcfile="$(pwd)/target/release/worm_cache"
    dstfile="$(pwd)/../bins/pf/worm_cache_${satcnt}_${rdwr}_${rot}_${pht}"
    cp $srcfile $dstfile
done