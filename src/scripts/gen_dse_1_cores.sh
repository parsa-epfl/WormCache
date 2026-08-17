#!/bin/bash

ncores=1

ref_BTB=2048    # entries
ref_SMS=4096   # entries, IMP: verify manually, this cannot be zero
ref_STLB=1024   # entries
ref_L1=64       # KB
ref_LLC=1024    # KB
ref_prefix="BTB_${ref_BTB}_SMS_${ref_SMS}_STLB_${ref_STLB}_L1_${ref_L1}_LLC_${ref_LLC}"

BTB_assoc=4     # Same for both ref and dst, IMP: verify manually
SMS_assoc=16    # Same for both ref and dst, IMP: verify manually
STLB_assoc=4    # Same for both ref and dst, IMP: verify manually
L1_assoc=4      # Same for both ref and dst, IMP: verify manually
LLC_assoc=16    # Same for both ref and dst, IMP: verify manually

# Ref parameters
ref_btb_sets=$((ref_BTB / BTB_assoc))
ref_sms_sets=$((ref_SMS / SMS_assoc))
ref_stlb_sets=$((ref_STLB / STLB_assoc))
ref_l1_sets=$((ref_L1 * 1024 / L1_assoc / 64))  # Convert KB to bytes and divide by associativity and line size (64B)
ref_llc_sets=${ref_LLC} # The conversion to sets is done is parameter.rs, so we can just use the KB value here

function replace() {
    local type=$1
    local btb=$2
    local sms=$3
    local stlb=$4
    local l1=$5
    local llc=$6

    ref_file="../ref_cfgs/${ncores}_core/parameter_${type}_${ref_prefix}.rs"
    prefix="BTB_${btb}_SMS_${sms}_STLB_${stlb}_L1_${l1}_LLC_${llc}"
    dst_file="../cfgs/${ncores}_core/parameter_${type}_${prefix}.rs"

    echo "Copying $ref_file to $dst_file"
    mkdir -p "$(dirname "$dst_file")"
    cp "$ref_file" "$dst_file"

    # Update BTB sets assuming 4-way set associative
    btb_sets=$((btb / BTB_assoc))
    sed -i -e "s/BTB_SET: usize = ${ref_btb_sets}/BTB_SET: usize = ${btb_sets}/g" "$dst_file"

    # Update SMS sets assuming 16-way set associative
    if [ "$sms" -eq 0 ]; then
        sed -i -e "s/SMS_PREFETCHING: bool = true;/SMS_PREFETCHING: bool = false;/g" "$dst_file"
    else
        sms_sets=$((sms / SMS_assoc))
        sed -i -e "s/PHT_SETS: usize = ${ref_sms_sets}/PHT_SETS: usize = ${sms_sets}/g" "$dst_file"
    fi

    # Update STLB sets assuming 4-way set associative
    stlb_sets=$((stlb / STLB_assoc))
    sed -i -e "s/STLB_SET: usize = ${ref_stlb_sets}/STLB_SET: usize = ${stlb_sets}/g" "$dst_file"

    # Update L1 sets assuming 4-way set associative
    l1_sets=$((l1 * 1024 / L1_assoc / 64))  # Convert KB to bytes and divide by associativity and line size (64B)
    sed -i -e "s/HARVARD_PRI_I_CACHE_SET: usize = ${ref_l1_sets}/HARVARD_PRI_I_CACHE_SET: usize = ${l1_sets}/g" "$dst_file"
    sed -i -e "s/HARVARD_PRI_D_CACHE_SET: usize = ${ref_l1_sets}/HARVARD_PRI_D_CACHE_SET: usize = ${l1_sets}/g" "$dst_file"
    
    # Update LLC sets assuming 16-way set associative
    llc_sets=${llc}
    sed -i -e "s/SHARED_CACHE_SET: usize = ${ref_llc_sets}/SHARED_CACHE_SET: usize = ${llc_sets}/g" "$dst_file"
}

stlb=1024                   # Fixed STLB size for 1-core DSE
llc=32768                   # Fixed LLC size for 1-core DSE

for btb in 256 512 1024 2048 4096 8192 16384; do
    for sms in 0 1024 2048 4096 8192 16384; do
        for l1 in 8 16 32 64; do
            for type in os an; do
                replace $type $btb $sms $stlb $l1 $llc
            done
        done
    done
done