#!/bin/bash

cd ..

# for type in os an
# do
#     for cnt in 1 2 4 8 16 32 64
#     do
#         for cache in 1 2 4 8 16 32 64 128 256 512 1024 2048 4096 8192 16384 32768 65536 131072 262144 524288 1048576
#         do
#             if [ "$type" = "os" ] && [ $cnt -eq 1 ]; then
#                 continue
#             fi
#             srcfile="$(pwd)/src/configs/${type}_${cnt}_${cache}.rs"
#             dstfile="$(pwd)/src/parameter.rs"
#             echo "Copying ${srcfile} to ${dstfile}"
#             cp $srcfile $dstfile

#             echo "Building ..."
#             cargo build --release

#             srcfile="$(pwd)/target/release/libworm_cache.so"
#             dstfile="$(pwd)/../bins/ws/${type}_${cnt}_${cache}.so"
#             echo "Copying ${srcfile} to ${dstfile}"
#             cp $srcfile $dstfile
#         done
#     done
# done

satcnt=true
rdwr=true
rot=true

for type in os an
do
    for region in 32 64
    do
        for pht in 16 32 64 128 256 512 1024
        do
            srcfile="$(pwd)/src/configs/parameter_${type}_${region}_${satcnt}_${rdwr}_${rot}_${pht}.rs"
            dstfile="$(pwd)/src/parameter.rs"
            echo "Copying ${srcfile} to ${dstfile}"
            cp $srcfile $dstfile

            echo "Building ..."
            cargo build --release --bin worm_cache

            srcfile="$(pwd)/target/release/worm_cache"
            dstfile="$(pwd)/../bins/pf/worm_cache_${type}_${region}_${satcnt}_${rdwr}_${rot}_${pht}"
            cp $srcfile $dstfile
        done
    done
done