#!/bin/bash

gen_new_file() {
    local type=$1
    local cores=$2
    local btb=$3
    local sms=$4
    local stlb=$5
    local l1=$6
    local llc=$7

    prefix="BTB_${btb}_SMS_${sms}_STLB_${stlb}_L1_${l1}_LLC_${llc}"
    ref_file="$(pwd)/../ref_files/parameter_${type}.rs"
    new_file="$(pwd)/../configs/${cores}_core/parameter_${type}_${prefix}.rs"
    cp $ref_file $new_file

    if [ "$type" == "os" ]; then
        local newCores=$(($cores * 2))
        sed -i -e "s/CORE_COUNT: usize = 2/CORE_COUNT: usize = ${newCores}/g" "$new_file"
    else
        local newCores=$cores
        sed -i -e "s/CORE_COUNT: usize = 1/CORE_COUNT: usize = ${newCores}/g" "$new_file"
    fi

    if [ "$sms" == "0" ]; then
        sed -i -e "s/SMS_PREFETCHING: bool = false/SMS_PREFETCHING: bool = false/g" "$new_file"
    else
        sed -i -e "s/SMS_PREFETCHING: bool = false/SMS_PREFETCHING: bool = true/g" "$new_file"
        PHT_SETS=$((sms / 16))  # Hardcoded  
        sed -i -e "s/PHT_SETS: usize = 256/PHT_SETS: usize = ${PHT_SETS}/g" "$new_file"
    fi

    STLB_SETS=$((stlb / 4))     # Hardcoded
    BTB_SETS=$((btb / 4))       # Hardcoded
    sed -i -e "s/STLB_SET: usize = 256/STLB_SET: usize = ${STLB_SETS}/g" "$new_file"
    sed -i -e "s/BTB_SET: usize = 4096/BTB_SET: usize = ${BTB_SETS}/g" "$new_file"
    sed -i -e "s/HARVARD_PRI_I_CACHE_SET: usize = 64/HARVARD_PRI_I_CACHE_SET: usize = ${l1}/g" "$new_file"
    sed -i -e "s/HARVARD_PRI_D_CACHE_SET: usize = 64/HARVARD_PRI_D_CACHE_SET: usize = ${l1}/g" "$new_file"
    sed -i -e "s/SHARED_CACHE_SET: usize = 32/SHARED_CACHE_SET: usize = ${llc}/g" "$new_file"    
}

for type in os an; do
    # Baseline: 4 cores, 8k BTB, 16k SMS, 1k STLB, 64KB L1, 16MB LLC
    cores=4
    btb=8192
    sms=16384
    stlb=1024
    l1=64
    llc=16
    gen_new_file "$type" "$cores" "$btb" "$sms" "$stlb" "$l1" "$llc"

    # Optimal OoO chiplet: 16 cores, 8k BTB, 16k SMS, 1k STLB, 64KB L1, 4MB LLC
    cores=16
    btb=8192
    sms=16384
    stlb=1024
    l1=64
    llc=4
    gen_new_file "$type" "$cores" "$btb" "$sms" "$stlb" "$l1" "$llc"

    # Optimal InO chiplet: 48 cores, 2k BTB, 4k SMS, 1k STLB, 64KB L1, 12MB LLC
    cores=48
    btb=2048
    sms=4096
    stlb=1024
    l1=64
    llc=12
    gen_new_file "$type" "$cores" "$btb" "$sms" "$stlb" "$l1" "$llc"
done