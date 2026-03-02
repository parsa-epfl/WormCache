#!/bin/bash

gen_new_bin() {
    local type=$1
    local cores=$2
    local btb=$3
    local sms=$4
    local stlb=$5
    local l1=$6
    local llc=$7

    prefix="BTB_${btb}_SMS_${sms}_STLB_${stlb}_L1_${l1}_LLC_${llc}"
    srcfile="$(pwd)/src/configs/${cores}_core/parameter_${type}_${prefix}.rs"
    dstfile="$(pwd)/src/parameter.rs"

    echo "Copying ${srcfile} to ${dstfile}"
    cp $srcfile $dstfile

    echo "Building ..."
    cargo build --release

    srcfile="$(pwd)/target/release/libworm_cache.so"
    dstfile="$(pwd)/../bins/${cores}_core/libworm_cache_${type}_${prefix}.so"
    echo "Copying ${srcfile} to ${dstfile}"
    cp $srcfile $dstfile

    srcfile="$(pwd)/target/release/checkpoint_conversion"
    dstfile="$(pwd)/../bins/${cores}_core/checkpoint_conversion_${type}_${prefix}"
    echo "Copying ${srcfile} to ${dstfile}"
    cp $srcfile $dstfile
}

cd ..

for type in os an; do
    # Baseline: 4 cores, 8k BTB, 16k SMS, 1k STLB, 64KB L1, 16MB LLC
    cores=4
    btb=8192
    sms=16384
    stlb=1024
    l1=64
    llc=16
    gen_new_bin "$type" "$cores" "$btb" "$sms" "$stlb" "$l1" "$llc"

    # Optimal OoO chiplet: 16 cores, 8k BTB, 16k SMS, 1k STLB, 64KB L1, 4MB LLC
    cores=16
    btb=8192
    sms=16384
    stlb=1024
    l1=64
    llc=4
    gen_new_bin "$type" "$cores" "$btb" "$sms" "$stlb" "$l1" "$llc"

    # Optimal InO chiplet: 48 cores, 2k BTB, 4k SMS, 1k STLB, 64KB L1, 12MB LLC
    cores=48
    btb=2048
    sms=4096
    stlb=1024
    l1=64
    llc=12
    gen_new_bin "$type" "$cores" "$btb" "$sms" "$stlb" "$l1" "$llc"
done