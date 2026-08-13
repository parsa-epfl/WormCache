#!/bin/bash

ncores=4
for tuple in "8192 16384" "2048 4096"; do
    read -r btb sms <<< "$tuple"
    for type in os an; do
        for llc in 64 128 256 512 1024 2048 4096 8192 16384; do
            ref_prefix="BTB_${btb}_SMS_${sms}_STLB_1024_L1_64_LLC_1024"
            dst_prefix="BTB_${btb}_SMS_${sms}_STLB_1024_L1_64_LLC_${llc}"

            ref_file="../ref_cfgs/${ncores}_core/parameter_${type}_${ref_prefix}.rs"
            dst_file="../cfgs/${ncores}_core/parameter_${type}_${dst_prefix}.rs"
            echo "Copying $ref_file to $dst_file"

            mkdir -p "$(dirname "$dst_file")"
            cp "$ref_file" "$dst_file"

            sed -i -e "s/SHARED_CACHE_SET: usize = 1024/SHARED_CACHE_SET: usize = ${llc}/g" "$dst_file"
        done
    done
done