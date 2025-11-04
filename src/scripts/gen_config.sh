
oldfile="$(pwd)/../configs/ref_parameter_sms.rs"

# for type in os an
# do
#     for cnt in 1 2 4 8 16 32 64
#     do
#         for cache in 1 2 4 8 16 32 64 128 256 512 1024 2048 4096 8192 16384 32768 65536 131072 262144 524288 1048576
#         do
#             newfile="$(pwd)/../configs/${type}_${cnt}_${cache}.rs"
#             if [ "$type" = "os" ] && [ $cnt -eq 1 ]; then
#                 continue
#             fi

#             cp $oldfile $newfile

#             sed -i -e "s/CORE_COUNT: usize = 1/CORE_COUNT: usize = ${cnt}/g" "$newfile"
#             if [ "$type" = "os" ]; then
#                 sed -i -e "s/MEASURE_HALF_OF_CORES: bool = false/MEASURE_HALF_OF_CORES: bool = true/g" "$newfile"
#             fi
            
#             sed -i -E "s/(SHARED_CACHE_SET: usize = )32/\1${cache}/" "$newfile"
#         done
#     done
# done

for type in os an
do
    for region in 2 4 8 16 32 64 128
    do
        for satcnt in true false
        do
            for rdwr in true false
            do
                for rot in true false
                do
                    for pht in 16 32 64 128 256 512 1024 perfect
                    do
                        newfile="$(pwd)/../configs/parameter_${type}_${region}_${satcnt}_${rdwr}_${rot}_${pht}.rs"
                        cp $oldfile $newfile

                        if [ "$type" = "os" ]; then
                            sed -i -e "s/CORE_COUNT: usize = 1/CORE_COUNT: usize = 2/g" "$newfile"
                            sed -i -e "s/MEASURE_HALF_OF_CORES: bool = false/MEASURE_HALF_OF_CORES: bool = true/g" "$newfile"
                        fi

                        sed -i -e "s/SAT_CNT: bool = false/SAT_CNT: bool = ${satcnt}/g" "$newfile"
                        sed -i -e "s/SEP_RDWR: bool = false/SEP_RDWR: bool = ${rdwr}/g" "$newfile"
                        sed -i -e "s/ROT: bool = false/ROT: bool = ${rot}/g" "$newfile"
                        sed -i -e "s/N_BLK: usize = 32/N_BLK: usize = ${region}/g" "$newfile"

                        if [ "$pht" = "perfect" ]; then
                            sed -i -e "s/PERFECT_PHT: bool = false/PERFECT_PHT: bool = true/g" "$newfile"
                        else
                            sed -i -e "s/PHT_SETS: usize = 256/PHT_SETS: usize = ${pht}/g" "$newfile"
                            sed -i -e "s/PERFECT_PHT: bool = false/PERFECT_PHT: bool = false/g" "$newfile"
                        fi
                    done
                done
            done
        done
    done
done