#!/bin/bash

pool=/mnt/pmem0/pmem.pool
full_path=$(realpath $0)
dir_path=$(dirname $full_path)

p=$(pwd)
cd $dir_path/..
cargo build --release --examples
cd $p

[ -f $dir_path/inputs.tar.gz ] && tar xzvf $dir_path/inputs.tar.gz -C $dir_path && rm -f $dir_path/inputs.tar.gz

ls -1 $dir_path/inputs/wc/* > $dir_path/files.list
mkdir -p $dir_path/outputs/wc

for r in 1 2; do
    for c in 1 2 4 8 16; do
        rm -f $pool
        echo "Running test $r:$c ..."
        perf stat -o $dir_path/outputs/wc/$r-$c.out -C 1-$(($r+$c)) $dir_path/../target/release/examples/grep -r $r -c $c -f $pool $dir_path/files.list > $dir_path/outputs/wc/$r-$c.res

        curl -X POST -H 'Content-type: application/json' --data "{\"text\":\"$r:$c Output
\`\`\``cat $dir_path/outputs/wc/$r-$c.out`\`\`\`\"}" https://hooks.slack.com/services/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
    done
done

function read_time() {
	echo $(cat $1 | grep -oP '(\d+\.\d+)\s+seconds time elapsed' | grep -oP '(\d+\.\d+)')
}	

echo "p/c,1,2,4,8,16," > $dir_path/outputs/scale.csv

for r in 1 2; do
	echo -n "$r,"
	for c in 1 2 4 8 16; do
		echo -n $(read_time "$dir_path/outputs/wc/$r-$c.out"),
	done
	echo
done >> $dir_path/outputs/scale.csv

cat $dir_path/outputs/scale.csv | column -t -s,
curl -X POST -H 'Content-type: application/json' --data "{\"text\":\"Final Scalability Output
\`\`\``cat $dir_path/outputs/scale.csv | column -t -s,`\`\`\`\"}" https://hooks.slack.com/services/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

ins=(INS CHK REM RAND)

rm -f $pool
for i in ${ins[@]}; do
PMEM_NO_CLWB=1 PMEM_NO_CLFLUSHOPT=1 PMEM_NO_MOVNT=1 PMEM_NO_FLUSH=0 perf stat -C 1 -o $dir_path/outputs/perf/pmdk-$i.out -d $dir_path/pmdk-1.8/src/examples/libpmemobj/map/mapcli btree $pool < $dir_path/inputs/perf/$i > /dev/null
done

rm -f $pool
CPMEM_NO_CLWB=1 PMEM_NO_CLFLUSHOPT=1 PMEM_NO_MOVNT=1 PMEM_NO_FLUSH=0 perf stat -C 1 -o $dir_path/outputs/perf/pmdk-bst-INS.out -d $dir_path/pmdk-1.8/src/examples/libpmemobj/btree $pool s 30000
CPMEM_NO_CLWB=1 PMEM_NO_CLFLUSHOPT=1 PMEM_NO_MOVNT=1 PMEM_NO_FLUSH=0 perf stat -C 1 -o $dir_path/outputs/perf/pmdk-bst-CHK.out -d $dir_path/pmdk-1.8/src/examples/libpmemobj/btree $pool r 30000

rm -f $pool
CPMEM_NO_CLWB=1 PMEM_NO_CLFLUSHOPT=1 PMEM_NO_MOVNT=1 PMEM_NO_FLUSH=0 perf stat -C 1 -o $dir_path/outputs/perf/pmdk-kv-PUT.out -d $dir_path/libpmemobj-cpp/build/examples/example-simplekv $pool burst put 100000
CPMEM_NO_CLWB=1 PMEM_NO_CLFLUSHOPT=1 PMEM_NO_MOVNT=1 PMEM_NO_FLUSH=0 perf stat -C 1 -o $dir_path/outputs/perf/pmdk-kv-GET.out -d $dir_path/libpmemobj-cpp/build/examples/example-simplekv $pool burst get 100000

rm -f $pool
for i in ${ins[@]}; do
CPUS=1 perf stat -C 1 -o $dir_path/outputs/perf/crndm-$i.out -d $dir_path/../target/release/examples/mapcli btree $pool < $dir_path/inputs/perf/$i > /dev/null
done

rm -f $pool
CPUS=1 perf stat -C 1 -o $dir_path/outputs/perf/crndm-bst-INS.out -d $dir_path/..target/release/examples/btree $pool s 30000
CPUS=1 perf stat -C 1 -o $dir_path/outputs/perf/crndm-bst-CHK.out -d $dir_path/..target/release/examples/btree $pool r 30000

rm -f $pool
PUS=1 perf stat -C 1 -o $dir_path/outputs/perf/crndm-kv-PUT.out -d $dir_path/..target/release/examples/simplekv $pool burst put 100000
PUS=1 perf stat -C 1 -o $dir_path/outputs/perf/crndm-kv-GET.out -d $dir_path/..target/release/examples/simplekv $pool burst get 100000

echo "Execution Time (s),,,,,,,,,"                                       > $dir_path/outputs/perf.csv
echo ",BST,,KVStore,,B+Tree,,,,"                                        >> $dir_path/outputs/perf.csv
echo ",INS,CHK,PUT,GET,INS,CHK,REM,RAND"                                >> $dir_path/outputs/perf.csv
echo -n PMDK,$(read_time "$dir_path/outputs/wc/pmdk-bst-INS.out"),      >> $dir_path/outputs/perf.csv
echo -n      $(read_time "$dir_path/outputs/wc/pmdk-bst-CHK.out"),      >> $dir_path/outputs/perf.csv
echo -n      $(read_time "$dir_path/outputs/wc/pmdk-kv-PUT.out"),       >> $dir_path/outputs/perf.csv
echo -n      $(read_time "$dir_path/outputs/wc/pmdk-kv-GET.out"),       >> $dir_path/outputs/perf.csv
echo -n      $(read_time "$dir_path/outputs/wc/pmdk-INS.out"),          >> $dir_path/outputs/perf.csv
echo -n      $(read_time "$dir_path/outputs/wc/pmdk-CHK.out"),          >> $dir_path/outputs/perf.csv
echo -n      $(read_time "$dir_path/outputs/wc/pmdk-REM.out"),          >> $dir_path/outputs/perf.csv
echo -n      $(read_time "$dir_path/outputs/wc/pmdk-RAND.out")          >> $dir_path/outputs/perf.csv
echo                                                                    >> $dir_path/outputs/perf.csv
echo -n Corundum,$(read_time "$dir_path/outputs/wc/pmdk-bst-INS.out"),  >> $dir_path/outputs/perf.csv
echo -n      $(read_time "$dir_path/outputs/wc/pmdk-bst-CHK.out"),      >> $dir_path/outputs/perf.csv
echo -n      $(read_time "$dir_path/outputs/wc/pmdk-kv-PUT.out"),       >> $dir_path/outputs/perf.csv
echo -n      $(read_time "$dir_path/outputs/wc/pmdk-kv-GET.out"),       >> $dir_path/outputs/perf.csv
echo -n      $(read_time "$dir_path/outputs/wc/pmdk-INS.out"),          >> $dir_path/outputs/perf.csv
echo -n      $(read_time "$dir_path/outputs/wc/pmdk-CHK.out"),          >> $dir_path/outputs/perf.csv
echo -n      $(read_time "$dir_path/outputs/wc/pmdk-REM.out"),          >> $dir_path/outputs/perf.csv
echo -n      $(read_time "$dir_path/outputs/wc/pmdk-RAND.out")          >> $dir_path/outputs/perf.csv
echo                                                                    >> $dir_path/outputs/perf.csv

cat $dir_path/outputs/perf.csv | column -t -s,
curl -X POST -H 'Content-type: application/json' --data "{\"text\":\"Final Scalability Output
\`\`\``cat $dir_path/outputs/perf.csv | column -t -s,`\`\`\`\"}" https://hooks.slack.com/services/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

