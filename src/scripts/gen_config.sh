
oldfile="$(pwd)/../configs/ref_parameter.rs"
for satcnt in true false
do
    for rdwr in true false
    do
        for rot in true false
        do
            for pht in 16 32 64 128 256 512 1024 perfect
            do
                newfile="$(pwd)/../configs/parameter_${satcnt}_${rdwr}_${rot}_${pht}.rs"
                cp $oldfile $newfile

                sed -i -e "s/SAT_CNT: bool = false/SAT_CNT: bool = ${satcnt}/g" "$newfile"
                sed -i -e "s/SEP_RDWR: bool = false/SEP_RDWR: bool = ${rdwr}/g" "$newfile"
                sed -i -e "s/ROT: bool = false/ROT: bool = ${rot}/g" "$newfile"

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