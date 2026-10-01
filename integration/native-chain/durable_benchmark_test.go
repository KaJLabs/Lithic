//go:build lithovm_chain_lab && linux

package nativechain

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strconv"
	"strings"
	"sync/atomic"
	"syscall"
	"testing"
	"time"

	"cosmossdk.io/log"
	"cosmossdk.io/store"
	"cosmossdk.io/store/metrics"
	storetypes "cosmossdk.io/store/types"
	"github.com/cometbft/cometbft/proto/tendermint/types"
	dbm "github.com/cosmos/cosmos-db"
	sdk "github.com/cosmos/cosmos-sdk/types"
	"github.com/ethereum/go-ethereum/common"
	"github.com/evmos/evmos/v20/x/evm/statedb"
)

// This measures isolated GoLevelDB/IAVL commit behavior, not a running validator,
// consensus, RPC, or the production block-gas envelope. It is opt-in because
// full 100k-holder/allowance population intentionally performs many calls.
func TestDurableTokenStateCandidate(t *testing.T) {
	if os.Getenv("LITHOVM_DURABLE_BENCH") != "1" {
		t.Skip("set LITHOVM_DURABLE_BENCH=1 for isolated durable-state measurements")
	}
	counts := []int{1_000, 10_000, 100_000}
	samples := 128
	if raw := os.Getenv("LITHOVM_BENCH_SAMPLES"); raw != "" {
		value, err := strconv.Atoi(raw)
		if err != nil || value < 1 || value > 1_000 {
			t.Fatalf("invalid benchmark sample count %q", raw)
		}
		samples = value
	}
	if raw := os.Getenv("LITHOVM_BENCH_COUNTS"); raw != "" {
		counts = nil
		for _, field := range strings.Split(raw, ",") {
			value, err := strconv.Atoi(strings.TrimSpace(field))
			if err != nil || value < 1 || value > 100_000 || (len(counts) != 0 && value <= counts[len(counts)-1]) {
				t.Fatalf("invalid ascending benchmark counts %q", raw)
			}
			counts = append(counts, value)
		}
	}
	for _, mode := range []string{"holders", "allowances", "mixed"} {
		t.Run(mode, func(t *testing.T) {
			runDurableTokenState(t, mode, counts, samples)
		})
	}
}

func runDurableTokenState(t *testing.T, mode string, counts []int, samples int) {
	t.Helper()
	dir := t.TempDir()
	backingDB, err := dbm.NewDB("native-state", dbm.GoLevelDBBackend, dir)
	if err != nil {
		t.Fatal(err)
	}
	physicalDB := &meteredDB{DB: backingDB}
	defer func() { physicalDB.Close() }()
	key := storetypes.NewKVStoreKey("lithovm")
	cms := store.NewCommitMultiStore(physicalDB, log.NewNopLogger(), metrics.NewNoOpMetrics())
	cms.MountStoreWithDB(key, storetypes.StoreTypeIAVL, physicalDB)
	if err := cms.LoadLatestVersion(); err != nil {
		t.Fatal(err)
	}
	ctx := sdk.NewContext(cms, types.Header{}, false, log.NewNopLogger())
	reopen := func() {
		t.Helper()
		if err := physicalDB.Close(); err != nil {
			t.Fatal(err)
		}
		backingDB, err = dbm.NewDB("native-state", dbm.GoLevelDBBackend, dir)
		if err != nil {
			t.Fatal(err)
		}
		physicalDB = &meteredDB{DB: backingDB}
		cms = store.NewCommitMultiStore(physicalDB, log.NewNopLogger(), metrics.NewNoOpMetrics())
		cms.MountStoreWithDB(key, storetypes.StoreTypeIAVL, physicalDB)
		if err := cms.LoadLatestVersion(); err != nil {
			t.Fatal(err)
		}
		ctx = sdk.NewContext(cms, types.Header{}, false, log.NewNopLogger())
	}
	env := Environment{ChainID: 700777, Height: 1, Timestamp: 2, Nonce: 1}
	deploy := request(t)
	deploy.Bytecode = ptr(fixture(t, "testdata/durable_token_state.lithic"))
	deploy.Arguments = "0x4c56414c010000"
	db := statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
	result, err := ExecuteFrame(db, key, frame(10_000_000), false, env, deploy)
	if err != nil || !result.Success {
		t.Fatalf("deploy: %v %+v", err, result)
	}
	if err := db.Commit(); err != nil {
		t.Fatal(err)
	}
	cms.Commit()
	contract := returnedAddress(t, result)
	call := deploy
	call.Operation = "call"
	call.Bytecode = nil
	call.Contract = &contract
	call.GasLimit = 10_000_000

	invoke := func(fn, arguments string, commit bool) (time.Duration, uint64) {
		t.Helper()
		call.Function = ptr(fn)
		call.Arguments = arguments
		frame := frame(10_000_000)
		db := statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
		start := time.Now()
		response, err := ExecuteFrame(db, key, frame, false, env, call)
		elapsed := time.Since(start)
		if err != nil || !response.Success {
			t.Fatalf("%s: %v %+v", fn, err, response)
		}
		if commit {
			if err := db.Commit(); err != nil {
				t.Fatal(err)
			}
		}
		return elapsed, 10_000_000 - frame.Gas
	}

	previous := 0
	for _, count := range counts {
		added := count - previous
		beforePopulate := processUsage(t)
		beforeDB := physicalDB.snapshot()
		populateStart := time.Now()
		for i := previous + 1; i <= count; i++ {
			if mode != "allowances" {
				invoke("set_balance", mapArgs(uint64(i), 1), true)
			}
			if mode != "holders" {
				invoke("approve", allowanceWriteArgs(uint64(i), uint64(i+100_000), 1), true)
			}
			if i%1_000 == 0 {
				cms.Commit()
			}
		}
		cms.Commit()
		populateDuration := time.Since(populateStart)
		afterPopulate := processUsage(t)
		afterDB := physicalDB.snapshot()
		previous = count
		reopen()
		reopenFunction, reopenArgs := "balance", addressArgs(1)
		if mode == "allowances" {
			reopenFunction, reopenArgs = "allowance", allowanceReadArgs(1, 100_001)
		}
		reopenRead, _ := invoke(reopenFunction, reopenArgs, false)
		bytes := directoryBytes(t, dir)
		logicalKeys, logicalBytes := nativeStateSize(t, ctx, key)
		t.Logf("DURABLE_FILL mode=%s entries=%d added=%d wall_ns=%d db_bytes=%d logical_keys=%d logical_bytes=%d rss_kb=%d max_rss_kb=%d cpu_user_ns=%d cpu_system_ns=%d db_gets=%d db_has=%d db_iterators=%d db_sets=%d db_deletes=%d db_batch_writes=%d process_read_bytes=%d process_write_bytes=%d process_read_syscalls=%d process_write_syscalls=%d",
			mode, count, added, populateDuration.Nanoseconds(), bytes, logicalKeys, logicalBytes,
			afterPopulate.rssKB, afterPopulate.maxRSSKB,
			afterPopulate.userNS-beforePopulate.userNS, afterPopulate.systemNS-beforePopulate.systemNS,
			afterDB.gets-beforeDB.gets, afterDB.has-beforeDB.has, afterDB.iterators-beforeDB.iterators,
			afterDB.sets-beforeDB.sets, afterDB.deletes-beforeDB.deletes, afterDB.batchWrites-beforeDB.batchWrites,
			afterPopulate.readBytes-beforePopulate.readBytes, afterPopulate.writeBytes-beforePopulate.writeBytes,
			afterPopulate.readSyscalls-beforePopulate.readSyscalls, afterPopulate.writeSyscalls-beforePopulate.writeSyscalls)
		var heap runtime.MemStats
		runtime.ReadMemStats(&heap)

		for _, operation := range []struct {
			name  string
			fn    string
			args  func(int) string
			write bool
		}{
			{"balance-read", "balance", func(int) string { return addressArgs(1) }, false},
			{"balance-overwrite", "set_balance", func(sample int) string { return mapArgs(1, uint64(sample+2)) }, true},
			{"balance-growth", "set_balance", func(sample int) string { return mapArgs(uint64(1_000_000+count*2_000+sample), 2) }, true},
			{"allowance-read", "allowance", func(int) string { return allowanceReadArgs(1, 100_001) }, false},
			{"allowance-overwrite", "approve", func(sample int) string { return allowanceWriteArgs(1, 100_001, uint64(sample+2)) }, true},
			{"allowance-growth", "approve", func(sample int) string {
				owner := uint64(1_000_000 + count*2_000 + sample)
				return allowanceWriteArgs(owner, owner+100_000, 2)
			}, true},
		} {
			if mode == "holders" && strings.HasPrefix(operation.name, "allowance") ||
				mode == "allowances" && strings.HasPrefix(operation.name, "balance") {
				continue
			}
			var execution, commits []time.Duration
			var gas uint64
			beforeBytes := directoryBytes(t, dir)
			beforeOperation := processUsage(t)
			beforeDB := physicalDB.snapshot()
			for sample := 0; sample < samples; sample++ {
				elapsed, used := invoke(operation.fn, operation.args(sample), operation.write)
				execution = append(execution, elapsed)
				if sample != 0 && used != gas {
					t.Fatalf("nondeterministic %s gas: first=%d current=%d", operation.name, gas, used)
				}
				gas = used
				if operation.write {
					start := time.Now()
					cms.Commit()
					commits = append(commits, time.Since(start))
				}
			}
			afterOperation := processUsage(t)
			afterDB := physicalDB.snapshot()
			t.Logf("DURABLE mode=%s entries=%d op=%s samples=%d reopen_read_ns=%d db_bytes=%d file_delta_bytes=%d heap_bytes=%d rss_kb=%d gas=%d execute_ns_p50=%d execute_ns_p95=%d execute_ns_p99=%d commit_ns_p50=%d commit_ns_p95=%d commit_ns_p99=%d db_gets=%d db_has=%d db_iterators=%d db_sets=%d db_deletes=%d db_batch_writes=%d process_read_bytes=%d process_write_bytes=%d process_read_syscalls=%d process_write_syscalls=%d",
				mode, count, operation.name, samples, reopenRead.Nanoseconds(), bytes,
				directoryBytes(t, dir)-beforeBytes, heap.Alloc, afterOperation.rssKB, gas,
				percentile(execution, 50), percentile(execution, 95), percentile(execution, 99),
				percentile(commits, 50), percentile(commits, 95), percentile(commits, 99),
				afterDB.gets-beforeDB.gets, afterDB.has-beforeDB.has, afterDB.iterators-beforeDB.iterators,
				afterDB.sets-beforeDB.sets, afterDB.deletes-beforeDB.deletes, afterDB.batchWrites-beforeDB.batchWrites,
				afterOperation.readBytes-beforeOperation.readBytes, afterOperation.writeBytes-beforeOperation.writeBytes,
				afterOperation.readSyscalls-beforeOperation.readSyscalls, afterOperation.writeSyscalls-beforeOperation.writeSyscalls)
		}
		endKeys, endBytes := nativeStateSize(t, ctx, key)
		t.Logf("DURABLE_STATE_END mode=%s entries=%d logical_keys=%d logical_bytes=%d db_bytes=%d", mode, count, endKeys, endBytes, directoryBytes(t, dir))
	}
}

func nativeStateSize(t *testing.T, ctx sdk.Context, key *storetypes.KVStoreKey) (int, int64) {
	t.Helper()
	iterator := ctx.KVStore(key).Iterator(nil, nil)
	defer iterator.Close()
	var entries int
	var bytes int64
	for ; iterator.Valid(); iterator.Next() {
		entries++
		bytes += int64(len(iterator.Key()) + len(iterator.Value()))
	}
	return entries, bytes
}

type dbCounts struct{ gets, has, iterators, sets, deletes, batchWrites uint64 }

type meteredDB struct {
	dbm.DB
	gets, has, iterators, sets, deletes, batchWrites atomic.Uint64
}

func (db *meteredDB) Get(key []byte) ([]byte, error) {
	db.gets.Add(1)
	return db.DB.Get(key)
}

func (db *meteredDB) Has(key []byte) (bool, error) {
	db.has.Add(1)
	return db.DB.Has(key)
}

func (db *meteredDB) Iterator(start, end []byte) (dbm.Iterator, error) {
	db.iterators.Add(1)
	return db.DB.Iterator(start, end)
}

func (db *meteredDB) ReverseIterator(start, end []byte) (dbm.Iterator, error) {
	db.iterators.Add(1)
	return db.DB.ReverseIterator(start, end)
}

func (db *meteredDB) Set(key, value []byte) error {
	db.sets.Add(1)
	return db.DB.Set(key, value)
}

func (db *meteredDB) SetSync(key, value []byte) error {
	db.sets.Add(1)
	return db.DB.SetSync(key, value)
}

func (db *meteredDB) Delete(key []byte) error {
	db.deletes.Add(1)
	return db.DB.Delete(key)
}

func (db *meteredDB) DeleteSync(key []byte) error {
	db.deletes.Add(1)
	return db.DB.DeleteSync(key)
}

func (db *meteredDB) NewBatch() dbm.Batch {
	return &meteredBatch{Batch: db.DB.NewBatch(), parent: db}
}

func (db *meteredDB) NewBatchWithSize(size int) dbm.Batch {
	return &meteredBatch{Batch: db.DB.NewBatchWithSize(size), parent: db}
}

func (db *meteredDB) snapshot() dbCounts {
	return dbCounts{db.gets.Load(), db.has.Load(), db.iterators.Load(), db.sets.Load(), db.deletes.Load(), db.batchWrites.Load()}
}

type meteredBatch struct {
	dbm.Batch
	parent *meteredDB
}

func (batch *meteredBatch) Set(key, value []byte) error {
	batch.parent.sets.Add(1)
	return batch.Batch.Set(key, value)
}

func (batch *meteredBatch) Delete(key []byte) error {
	batch.parent.deletes.Add(1)
	return batch.Batch.Delete(key)
}

func (batch *meteredBatch) Write() error {
	batch.parent.batchWrites.Add(1)
	return batch.Batch.Write()
}

func (batch *meteredBatch) WriteSync() error {
	batch.parent.batchWrites.Add(1)
	return batch.Batch.WriteSync()
}

type usage struct {
	rssKB, maxRSSKB, readBytes, writeBytes, readSyscalls, writeSyscalls uint64
	userNS, systemNS                                                    int64
}

func processUsage(t *testing.T) usage {
	t.Helper()
	var result usage
	data, err := os.ReadFile("/proc/self/io")
	if err != nil {
		t.Fatal(err)
	}
	for _, line := range strings.Split(string(data), "\n") {
		parts := strings.Fields(line)
		if len(parts) != 2 {
			continue
		}
		value, err := strconv.ParseUint(parts[1], 10, 64)
		if err != nil {
			t.Fatal(err)
		}
		switch parts[0] {
		case "read_bytes:":
			result.readBytes = value
		case "write_bytes:":
			result.writeBytes = value
		case "syscr:":
			result.readSyscalls = value
		case "syscw:":
			result.writeSyscalls = value
		}
	}
	data, err = os.ReadFile("/proc/self/status")
	if err != nil {
		t.Fatal(err)
	}
	for _, line := range strings.Split(string(data), "\n") {
		if strings.HasPrefix(line, "VmRSS:") {
			parts := strings.Fields(line)
			result.rssKB, err = strconv.ParseUint(parts[1], 10, 64)
			if err != nil {
				t.Fatal(err)
			}
			break
		}
	}
	var rusage syscall.Rusage
	if err := syscall.Getrusage(syscall.RUSAGE_SELF, &rusage); err != nil {
		t.Fatal(err)
	}
	result.maxRSSKB = uint64(rusage.Maxrss)
	result.userNS = rusage.Utime.Sec*1_000_000_000 + rusage.Utime.Usec*1_000
	result.systemNS = rusage.Stime.Sec*1_000_000_000 + rusage.Stime.Usec*1_000
	return result
}

func addressArgs(address uint64) string {
	return fmt.Sprintf("0x4c56414c01000104%064x", address)
}

func allowanceReadArgs(owner, spender uint64) string {
	return fmt.Sprintf("0x4c56414c01000204%064x04%064x", owner, spender)
}

func allowanceWriteArgs(owner, spender, amount uint64) string {
	return fmt.Sprintf("0x4c56414c01000304%064x04%064x02%064x", owner, spender, amount)
}

func percentile(samples []time.Duration, percent int) int64 {
	if len(samples) == 0 {
		return 0
	}
	ordered := append([]time.Duration(nil), samples...)
	sort.Slice(ordered, func(i, j int) bool { return ordered[i] < ordered[j] })
	index := (len(ordered)*percent+99)/100 - 1
	return ordered[index].Nanoseconds()
}

func directoryBytes(t *testing.T, root string) int64 {
	t.Helper()
	var total int64
	err := filepath.Walk(root, func(_ string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}
		if info.Mode().IsRegular() {
			total += info.Size()
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	return total
}
