#!/bin/bash

cd ..

ncores=4
for type in os an; do
    for btb in 256 512 1024 2048 4096 8192 16384; do
        for sms in 0 1024 2048 4096 8192 16384; do
            for stlb in 256 512 1024 2048 4096 8192 16384; do
                for l1 in 8 16 32; do
                    for llc in 64 128 256 512 1024 2048 4096 8192 16384; do
                        prefix="BTB_${btb}_SMS_${sms}_STLB_${stlb}_L1_${l1}_LLC_${llc}"

                        src_file="$(pwd)/src/cfgs/${ncores}_core/parameter_${type}_${prefix}.rs"
                        dst_file="$(pwd)/src/parameter.rs"

                        echo "Copying ${src_file} to ${dst_file}"
                        cp $src_file $dst_file

                        echo "Building ..."
                        cargo build --release

                        src_file="$(pwd)/target/release/libworm_cache.so"
                        dst_file="$(pwd)/../bins/${ncores}_core/libworm_cache_${type}_${prefix}.so"
                        echo "Copying ${src_file} to ${dst_file}"
                        cp $src_file $dst_file

                        src_file="$(pwd)/target/release/checkpoint_conversion"
                        dst_file="$(pwd)/../bins/${ncores}_core/checkpoint_conversion_${type}_${prefix}"
                        echo "Copying ${src_file} to ${dst_file}"
                        cp $src_file $dst_file
                    done
                done
            done
        done
    done
done