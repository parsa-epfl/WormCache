#!/bin/bash

BTB=16384
BTB_SETS=$((BTB / 4))  # Hardcoded
for type in os an; do
    for sms in 0 1024 2048 4096 8192 16384; do
        for stlb in 256 512 1024 2048 4096; do
            for l1 in 8 16 32 64; do
                for llc in 1 8 32; do
                    prefix="SMS_${sms}_STLB_${stlb}_L1_${l1}_LLC_${llc}"

                    ref_file="$(pwd)/../ref_files/parameter_${type}.rs"
                    new_file="$(pwd)/../configs/single_core/parameter_${type}_${prefix}.rs"
                    cp $ref_file $new_file

                    if [ "$sms" == "0" ]; then
                        sed -i -e "s/SMS_PREFETCHING: bool = false/SMS_PREFETCHING: bool = false/g" "$new_file"
                    else
                        sed -i -e "s/SMS_PREFETCHING: bool = false/SMS_PREFETCHING: bool = true/g" "$new_file"
                        PHT_SETS=$((sms / 16))  # Hardcoded  
                        sed -i -e "s/PHT_SETS: usize = 256/PHT_SETS: usize = ${PHT_SETS}/g" "$new_file"
                    fi

                    STLB_SETS=$((stlb / 4))     # Hardcoded
                    sed -i -e "s/STLB_SET: usize = 256/STLB_SET: usize = ${STLB_SETS}/g" "$new_file"
                    sed -i -e "s/BTB_SET: usize = 4096/BTB_SET: usize = ${BTB_SETS}/g" "$new_file"
                    sed -i -e "s/HARVARD_PRI_I_CACHE_SET: usize = 64/HARVARD_PRI_I_CACHE_SET: usize = ${l1}/g" "$new_file"
                    sed -i -e "s/HARVARD_PRI_D_CACHE_SET: usize = 64/HARVARD_PRI_D_CACHE_SET: usize = ${l1}/g" "$new_file"
                    sed -i -e "s/SHARED_CACHE_SET: usize = 32/SHARED_CACHE_SET: usize = ${llc}/g" "$new_file"
                done
            done
        done
    done
done