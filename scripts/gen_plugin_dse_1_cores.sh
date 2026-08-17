#!/bin/bash

cd ..
ncores=1

function gen() {
    local type=$1
    local btb=$2
    local sms=$3
    local stlb=$4
    local l1=$5
    local llc=$6
    local force=$7

    prefix="BTB_${btb}_SMS_${sms}_STLB_${stlb}_L1_${l1}_LLC_${llc}"
    if [ -f "$(pwd)/../bins/${ncores}_core/libworm_cache_${type}_${prefix}.so" ] && [ "$force" != "true" ]; then
        echo "Skipping ${type} ${prefix} as it already exists."
        return
    fi

    src_file="$(pwd)/src/cfgs/${ncores}_core/parameter_${type}_${prefix}.rs"
    dst_file="$(pwd)/src/parameter.rs"

    echo "Copying ${src_file} to ${dst_file}"
    cp $src_file $dst_file

    echo "Building ..."
    cargo build --release

    src_file="$(pwd)/target/release/libworm_cache.so"
    dst_file="$(pwd)/../bins/${ncores}_core/libworm_cache_${type}_${prefix}.so"
    echo "Copying ${src_file} to ${dst_file}"
    mkdir -p "$(dirname "$dst_file")"
    cp $src_file $dst_file

    src_file="$(pwd)/target/release/checkpoint_conversion"
    dst_file="$(pwd)/../bins/${ncores}_core/checkpoint_conversion_${type}_${prefix}"
    echo "Copying ${src_file} to ${dst_file}"
    mkdir -p "$(dirname "$dst_file")"
    cp $src_file $dst_file
}

stlb=1024                   # Fixed STLB size for 1-core DSE
llc=32768                   # Fixed LLC size for 1-core DSE

for btb in 256 512 1024 2048 4096 8192 16384; do
    for sms in 0 1024 2048 4096 8192 16384; do
        for l1 in 8 16 32 64; do
            for type in os an; do
                gen $type $btb $sms $stlb $l1 $llc
            done
        done
    done
done