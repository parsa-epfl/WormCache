#!/bin/bash

cd ..

for type in os an; do
    for sms in 0 1024 2048 4096 8192 16384; do
        for stlb in 256 512 1024 2048 4096; do
            for l1 in 8 16 32 64; do
                for llc in 1 8 32; do
                    prefix="SMS_${sms}_STLB_${stlb}_L1_${l1}_LLC_${llc}"

                    srcfile="$(pwd)/src/configs/single_core/parameter_${type}_${prefix}.rs"
                    dstfile="$(pwd)/src/parameter.rs"

                    echo "Copying ${srcfile} to ${dstfile}"
                    cp $srcfile $dstfile

                    echo "Building ..."
                    cargo build --release

                    # srcfile="$(pwd)/target/release/libworm_cache.so"
                    # dstfile="$(pwd)/../bins/single_core/libworm_cache_${type}_${prefix}.so"
                    # echo "Copying ${srcfile} to ${dstfile}"
                    # cp $srcfile $dstfile

                    # srcfile="$(pwd)/target/release/checkpoint_conversion"
                    # dstfile="$(pwd)/../bins/single_core/checkpoint_conversion_${type}_${prefix}"
                    # echo "Copying ${srcfile} to ${dstfile}"
                    # cp $srcfile $dstfile

                    srcfile="$(pwd)/target/release/worm_cache"
                    dstfile="$(pwd)/../bins/trace/worm_cache_${type}_${prefix}"
                    echo "Copying ${srcfile} to ${dstfile}"
                    cp $srcfile $dstfile

                    # srcfile="$(pwd)/target/release/libworm_cache.so"
                    # dstfile="$(pwd)/../bins/trace/libworm_cache_${type}_${prefix}.so"
                    # echo "Copying ${srcfile} to ${dstfile}"
                    # cp $srcfile $dstfile
                done
            done
        done
    done
done