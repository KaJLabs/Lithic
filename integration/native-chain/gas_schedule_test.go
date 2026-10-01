//go:build lithovm_chain_lab

package nativechain

import (
	storetypes "cosmossdk.io/store/types"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/cosmos/cosmos-sdk/testutil"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/params"
	"github.com/evmos/evmos/v20/x/evm/core/vm"
	"github.com/evmos/evmos/v20/x/evm/statedb"
	"math"
	"strconv"
	"strings"
	"testing"
)

func TestNewStatePaysKVAndGrowthFloor(t *testing.T) {
	key := storetypes.NewKVStoreKey("gas-pricing")
	ctx := testutil.DefaultContext(key, storetypes.NewTransientStoreKey("gas-pricing-transient"))
	db := statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
	f := frame(10_000_000)
	result, err := ExecuteFrame(db, key, f, false, Environment{ChainID: 700777, Height: 1, Timestamp: 2, Nonce: 1}, request(t))
	if err != nil || !result.Success {
		t.Fatalf("deploy: %v %+v", err, result)
	}
	writes, err := result.DecodedWrites()
	if err != nil {
		t.Fatal(err)
	}
	var floor uint64
	cfg := storetypes.KVGasConfig()
	for k, v := range writes {
		size := uint64(len(k) + len(v))
		floor += cfg.WriteCostFlat + cfg.WriteCostPerByte*size
		// Candidate persistent-growth floor: rounded 32-byte allocation units.
		floor += ((size + 31) / 32) * params.SstoreSetGasEIP2200
	}
	if used := uint64(10_000_000) - f.Gas; used < floor {
		t.Fatalf("new state underpriced: charged=%d minimum-write-and-growth=%d", used, floor)
	}
}

func TestCandidateRatesMatchPinnedBaselinesAndDoNotOverflow(t *testing.T) {
	cfg := storetypes.KVGasConfig()
	if candidateReadFlat != cfg.ReadCostFlat || candidateReadByte != cfg.ReadCostPerByte || candidateWriteFlat != cfg.WriteCostFlat || candidateWriteByte != cfg.WriteCostPerByte || candidateLogFlat != params.LogGas+params.LogTopicGas || candidateLogByte != params.LogDataGas || candidateGrowthUnit != params.SstoreSetGasEIP2200 {
		t.Fatal("dependency baseline changed; explicit schedule review required")
	}
	if gasCost(1, math.MaxUint64, 2) != math.MaxUint64 || gasSum(math.MaxUint64, 1) != math.MaxUint64 {
		t.Fatal("gas arithmetic wrapped")
	}
	for _, c := range []struct{ old, next, want uint64 }{{0, 1, 20000}, {0, 32, 20000}, {32, 33, 20000}, {33, 63, 0}, {64, 1, 0}, {32, 32, 0}} {
		if got := growthGas(c.old, c.next); got != c.want {
			t.Fatalf("growth %d -> %d: %d != %d", c.old, c.next, got, c.want)
		}
	}
}

func TestNativeEnvelopeExhaustionBurnsEVMFrame(t *testing.T) {
	// Evmos EVM.Call zeroes remaining gas on any non-REVERT error. Keep
	// the bridge's direct-frame behavior aligned with that boundary: a
	// 10M native cap is not a partial-gas refund for an OOG transaction.
	f := frame(30_000_000)
	meter := frameMeter{frame: f, remaining: nativeExecutionGasCap}
	if err := meter.charge(nativeExecutionGasCap); err != nil {
		t.Fatal(err)
	}
	if f.Gas != 20_000_000 || meter.remaining != 0 {
		t.Fatalf("unexpected gas before cap exhaustion: frame=%d native=%d", f.Gas, meter.remaining)
	}
	if err := meter.charge(1); !errors.Is(err, vm.ErrOutOfGas) || f.Gas != 0 || !meter.exhausted {
		t.Fatalf("native envelope must fail as EVM OOG and burn frame: err=%v gas=%d", err, f.Gas)
	}
}

func TestExactDeploymentGasBoundaryAndNoEffectsOnExhaustion(t *testing.T) {
	req := request(t)
	var exact uint64
	for step := 0; step < 4; step++ {
		// First measure, then exact budget, then one gas below, then no lookup budget.
		budget := uint64(10_000_000)
		switch step {
		case 1:
			budget = exact
		case 2:
			budget = exact - 1
		case 3:
			budget = 1
		}
		key := storetypes.NewKVStoreKey("boundary")
		ctx := testutil.DefaultContext(key, storetypes.NewTransientStoreKey("boundary-t"))
		db := statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
		f := frame(budget)
		result, err := ExecuteFrame(db, key, f, false, Environment{ChainID: 700777, Height: 1, Timestamp: 2, Nonce: 1}, req)
		if step == 0 {
			if err != nil {
				t.Fatal(err)
			}
			exact = budget - f.Gas
			continue
		}
		if step == 1 {
			if err != nil || !result.Success || f.Gas != 0 {
				t.Fatalf("exact budget failed: %v gas=%d", err, f.Gas)
			}
			continue
		}
		if !errors.Is(err, vm.ErrOutOfGas) || f.Gas != 0 || result.Success || len(db.Logs()) != 0 {
			t.Fatalf("OOG leaked or misclassified: %v %+v", err, result)
		}
		cache, _ := db.GetCacheContext()
		it := cache.KVStore(key).Iterator(nil, nil)
		exists := it.Valid()
		it.Close()
		if exists {
			t.Fatal("OOG retained storage")
		}
	}
}

func TestGetterPaysReadsButNoWriteOrEmptyLog(t *testing.T) {
	key := storetypes.NewKVStoreKey("getter-pricing")
	ctx := testutil.DefaultContext(key, storetypes.NewTransientStoreKey("getter-t"))
	db := statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
	req := request(t)
	env := Environment{ChainID: 700777, Height: 1, Timestamp: 2, Nonce: 1}
	result, err := ExecuteFrame(db, key, frame(10_000_000), false, env, req)
	if err != nil {
		t.Fatal(err)
	}
	writes, _ := result.DecodedWrites()
	var size uint64
	for k, v := range writes {
		size += uint64(len(k) + len(v))
	}
	if err := db.Commit(); err != nil {
		t.Fatal(err)
	}
	contract := returnedAddress(t, result)
	req.Operation = "call"
	req.Bytecode = nil
	req.Contract = &contract
	req.Function = ptr("get")
	req.Arguments = args()
	db = statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
	f := frame(10_000_000)
	result, err = ExecuteFrame(db, key, f, false, env, req)
	if err != nil {
		t.Fatal(err)
	}
	native, _ := strconv.ParseUint(result.GasUsed, 10, 64)
	// One logical record load uses two callbacks (size then data), each a Get.
	expected := native + 2*gasCost(candidateReadFlat, candidateReadByte, size)
	if used := uint64(10_000_000) - f.Gas; used != expected || len(db.Logs()) != 0 {
		t.Fatalf("getter gas=%d expected=%d logs=%d", used, expected, len(db.Logs()))
	}
	req.Function = ptr("set")
	req.Arguments = args(7, 1) // unchanged value, but emits a log
	db = statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
	f = frame(10_000_000)
	result, err = ExecuteFrame(db, key, f, false, env, req)
	if err != nil {
		t.Fatal(err)
	}
	native, _ = strconv.ParseUint(result.GasUsed, 10, 64)
	logs, _ := json.Marshal(struct{ Deployments, Events []json.RawMessage }{result.Deployments, result.Events})
	expected = native + 2*gasCost(candidateReadFlat, candidateReadByte, size) + gasCost(candidateLogFlat, candidateLogByte, uint64(len(logs)))
	if used := uint64(10_000_000) - f.Gas; used != expected || len(db.Logs()) != 1 {
		t.Fatalf("log pricing gas=%d expected=%d", used, expected)
	}
}

func TestLiveFuelSharesReadAndVMEnvelopeAtExactBoundary(t *testing.T) {
	key := storetypes.NewKVStoreKey("live-fuel-boundary")
	ctx := testutil.DefaultContext(key, storetypes.NewTransientStoreKey("live-fuel-boundary-t"))
	env := Environment{ChainID: 700777, Height: 1, Timestamp: 2, Nonce: 1}
	db := statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
	deployed, err := ExecuteFrame(db, key, frame(10_000_000), false, env, request(t))
	if err != nil || !deployed.Success {
		t.Fatalf("deploy: %v %+v", err, deployed)
	}
	writes, err := deployed.DecodedWrites()
	if err != nil || len(writes) != 1 {
		t.Fatalf("deploy writes: %v %d", err, len(writes))
	}
	if err := db.Commit(); err != nil {
		t.Fatal(err)
	}
	var readCost uint64
	for k, v := range writes {
		readCost = 2 * gasCost(candidateReadFlat, candidateReadByte, uint64(len(k)+len(v)))
	}
	contract := returnedAddress(t, deployed)
	req := request(t)
	req.Operation, req.Bytecode, req.Contract, req.Function, req.Arguments = "call", nil, &contract, ptr("get"), args()
	measureDB := statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
	measured, err := ExecuteFrame(measureDB, key, frame(10_000_000), false, env, req)
	if err != nil || !measured.Success {
		t.Fatalf("measure getter: %v %+v", err, measured)
	}
	nativeGas, err := strconv.ParseUint(measured.GasUsed, 10, 64)
	if err != nil || nativeGas == 0 {
		t.Fatalf("invalid native gas: %v %s", err, measured.GasUsed)
	}
	exact := readCost + nativeGas
	for _, test := range []struct {
		budget uint64
		ok     bool
	}{
		{exact - 1, false},
		{exact, true},
	} {
		db = statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
		f := frame(test.budget)
		result, err := ExecuteFrame(db, key, f, false, env, req)
		if test.ok {
			if err != nil || !result.Success || f.Gas != 0 {
				t.Fatalf("exact shared budget failed: %v %+v remaining=%d", err, result, f.Gas)
			}
		} else if !errors.Is(err, vm.ErrOutOfGas) || result.Success || f.Gas != 0 || len(db.Logs()) != 0 {
			t.Fatalf("sub-boundary must revert: %v %+v remaining=%d", err, result, f.Gas)
		}
	}
}

func TestNativeKeysAreCanonicalAndNamespaceRestricted(t *testing.T) {
	for _, key := range []string{"", "other/" + strings.Repeat("0", 40), "lithovm/v1/contracts/" + strings.Repeat("A", 40), "lithovm/v1/contracts/" + strings.Repeat("z", 40), "lithovm/v1/contracts/00"} {
		if validStateKey(key) {
			t.Fatalf("accepted %q", key)
		}
	}
	if !validStateKey("lithovm/v1/contracts/" + strings.Repeat("a", 40)) {
		t.Fatal("rejected canonical key")
	}
	validMap := "lithovm/v2/maps/" + strings.Repeat("a", 40) + "/balances/" + strings.Repeat("0", 64)
	if !validStateKey(validMap) {
		t.Fatal("rejected canonical map key")
	}
	deleted, err := (Response{Writes: []Write{{Key: validMap, Delete: true}}}).DecodedWrites()
	if err != nil || deleted[validMap] != nil {
		t.Fatalf("canonical map deletion rejected: %v", err)
	}
	if _, err := (Response{Writes: []Write{{Key: "lithovm/v1/contracts/" + strings.Repeat("a", 40), Delete: true}}}).DecodedWrites(); err == nil {
		t.Fatal("contract-record deletion accepted")
	}
	if _, err := (Response{Writes: []Write{{Key: validMap, Value: "0x"}}}).DecodedWrites(); err == nil {
		t.Fatal("empty map value accepted as write")
	}
	for _, invalid := range []string{
		validMap + "f", strings.Replace(validMap, "/balances/", "/bad.field/", 1),
		strings.Replace(validMap, "/balances/", "/1balance/", 1),
		strings.Replace(validMap, "/balances/", "//", 1), strings.ToUpper(validMap),
		"lithovm/v2/maps/" + strings.Repeat("a", 40) + "/balances/00",
	} {
		if validStateKey(invalid) {
			t.Fatalf("accepted invalid map key %q", invalid)
		}
	}
}

func TestPerKeyMapStateStaysConstantSizeAcrossThousandHolders(t *testing.T) {
	key := storetypes.NewKVStoreKey("per-key-holders")
	ctx := testutil.DefaultContext(key, storetypes.NewTransientStoreKey("per-key-holders-t"))
	env := Environment{ChainID: 700777, Height: 1, Timestamp: 2, Nonce: 1}
	db := statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
	req := request(t)
	req.Bytecode = ptr(fixture(t, "testdata/storage_gas.lithic"))
	req.Function, req.Arguments = ptr("initialize"), args()
	deployed, err := ExecuteFrame(db, key, frame(10_000_000), false, env, req)
	if err != nil || !deployed.Success {
		t.Fatalf("deploy: %v %+v", err, deployed)
	}
	writes, err := deployed.DecodedWrites()
	if err != nil || len(writes) != 1 {
		t.Fatalf("deploy writes: %v %d", err, len(writes))
	}
	var record []byte
	for _, value := range writes {
		record = value
	}
	if err := db.Commit(); err != nil {
		t.Fatal(err)
	}
	contract := returnedAddress(t, deployed)
	req.Operation, req.Bytecode, req.Contract, req.Function = "call", nil, &contract, ptr("write")
	var firstGas uint64
	for holder := 1; holder <= 1000; holder++ {
		req.Arguments = fmt.Sprintf("0x4c56414c01000204%064x02%064x", holder, holder+1000)
		db = statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
		f := frame(10_000_000)
		result, err := ExecuteFrame(db, key, f, false, env, req)
		if err != nil || !result.Success {
			t.Fatalf("holder %d: %v %+v", holder, err, result)
		}
		writes, err := result.DecodedWrites()
		if err != nil || len(writes) != 2 {
			t.Fatalf("holder %d writes: %v %d", holder, err, len(writes))
		}
		mapWrites := 0
		for name, value := range writes {
			if strings.HasPrefix(name, "lithovm/v2/maps/") {
				mapWrites++
				if len(value) != 32 {
					t.Fatalf("holder %d map value length %d", holder, len(value))
				}
			} else if string(value) != string(record) {
				t.Fatalf("holder %d changed contract metadata", holder)
			}
		}
		if mapWrites != 1 {
			t.Fatalf("holder %d wrote %d map keys", holder, mapWrites)
		}
		used := uint64(10_000_000) - f.Gas
		if holder == 1 {
			firstGas = used
		} else if used != firstGas {
			t.Fatalf("holder %d gas %d differs from first %d", holder, used, firstGas)
		}
		if err := db.Commit(); err != nil {
			t.Fatal(err)
		}
	}
	req.Arguments = fmt.Sprintf("0x4c56414c01000204%064x02%064x", 1001, 2001)
	db = statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
	f := frame(firstGas - 1)
	failed, err := ExecuteFrame(db, key, f, false, env, req)
	if !errors.Is(err, vm.ErrOutOfGas) || failed.Success || f.Gas != 0 || len(db.Logs()) != 0 {
		t.Fatalf("new holder OOG leaked effects: %v %+v remaining=%d", err, failed, f.Gas)
	}
	req.Function = ptr("read")
	req.Arguments = fmt.Sprintf("0x4c56414c01000104%064x", 1)
	db = statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
	read, err := ExecuteFrame(db, key, frame(10_000_000), false, env, req)
	if err != nil || !read.Success || !strings.HasSuffix(read.Result, fmt.Sprintf("%064x", 1001)) {
		t.Fatalf("first holder not retained: %v %+v", err, read)
	}
	req.Arguments = fmt.Sprintf("0x4c56414c01000104%064x", 1001)
	db = statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
	missing, err := ExecuteFrame(db, key, frame(10_000_000), false, env, req)
	if err != nil || !missing.Success || !strings.HasSuffix(missing.Result, strings.Repeat("0", 64)) {
		t.Fatalf("OOG holder persisted: %v %+v", err, missing)
	}
}

func TestPerKeyCorruptionFailsClosedAndOverwriteRecovers(t *testing.T) {
	key := storetypes.NewKVStoreKey("per-key-corruption")
	ctx := testutil.DefaultContext(key, storetypes.NewTransientStoreKey("per-key-corruption-t"))
	env := Environment{ChainID: 700777, Height: 1, Timestamp: 2, Nonce: 1}
	db := statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
	req := request(t)
	req.Bytecode, req.Function, req.Arguments = ptr(fixture(t, "testdata/storage_gas.lithic")), ptr("initialize"), args()
	deployed, err := ExecuteFrame(db, key, frame(10_000_000), false, env, req)
	if err != nil || !deployed.Success {
		t.Fatalf("deploy: %v %+v", err, deployed)
	}
	if err := db.Commit(); err != nil {
		t.Fatal(err)
	}
	contract := returnedAddress(t, deployed)
	req.Operation, req.Bytecode, req.Contract, req.Function = "call", nil, &contract, ptr("write")
	req.Arguments = fmt.Sprintf("0x4c56414c01000204%064x02%064x", 7, 42)
	db = statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
	written, err := ExecuteFrame(db, key, frame(10_000_000), false, env, req)
	if err != nil || !written.Success {
		t.Fatalf("write: %v %+v", err, written)
	}
	writes, err := written.DecodedWrites()
	if err != nil {
		t.Fatal(err)
	}
	var mapKey string
	var mapValue []byte
	for name, value := range writes {
		if strings.HasPrefix(name, "lithovm/v2/maps/") {
			mapKey, mapValue = name, value
		}
	}
	if mapKey == "" {
		t.Fatal("map write missing")
	}
	if err := db.Commit(); err != nil {
		t.Fatal(err)
	}
	ctx.KVStore(key).Set([]byte(mapKey), []byte{1, 2})
	req.Function = ptr("read")
	req.Arguments = fmt.Sprintf("0x4c56414c01000104%064x", 7)
	db = statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
	corrupt, err := ExecuteFrame(db, key, frame(10_000_000), false, env, req)
	if !errors.Is(err, vm.ErrExecutionReverted) || corrupt.Success || len(corrupt.Writes) != 0 {
		t.Fatalf("corrupt map value escaped: %v %+v", err, corrupt)
	}
	ctx.KVStore(key).Set([]byte(mapKey), mapValue)
	db = statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
	recovered, err := ExecuteFrame(db, key, frame(10_000_000), false, env, req)
	if err != nil || !recovered.Success || !strings.HasSuffix(recovered.Result, fmt.Sprintf("%064x", 42)) {
		t.Fatalf("recovery failed: %v %+v", err, recovered)
	}
	req.Function = ptr("write")
	req.Arguments = fmt.Sprintf("0x4c56414c01000204%064x02%064x", 7, 43)
	db = statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
	overwritten, err := ExecuteFrame(db, key, frame(10_000_000), false, env, req)
	if err != nil || !overwritten.Success {
		t.Fatalf("overwrite failed: %v %+v", err, overwritten)
	}
	if err := db.Commit(); err != nil {
		t.Fatal(err)
	}
	if value := ctx.KVStore(key).Get([]byte(mapKey)); len(value) != 32 || value[31] != 43 {
		t.Fatalf("overwrite not durable: %x", value)
	}
	req.Arguments = fmt.Sprintf("0x4c56414c01000204%064x02%064x", 7, 0)
	db = statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
	deleted, err := ExecuteFrame(db, key, frame(10_000_000), false, env, req)
	if err != nil || !deleted.Success {
		t.Fatalf("delete failed: %v %+v", err, deleted)
	}
	deletions, err := deleted.DecodedWrites()
	value, present := deletions[mapKey]
	if err != nil || !present || value != nil {
		t.Fatalf("map deletion not encoded: %v %+v", err, deleted.Writes)
	}
	if err := db.Commit(); err != nil {
		t.Fatal(err)
	}
	if ctx.KVStore(key).Has([]byte(mapKey)) {
		t.Fatal("zero balance retained persistent map key")
	}
	req.Function, req.Arguments = ptr("read"), fmt.Sprintf("0x4c56414c01000104%064x", 7)
	db = statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
	zero, err := ExecuteFrame(db, key, frame(10_000_000), false, env, req)
	if err != nil || !zero.Success || !strings.HasSuffix(zero.Result, strings.Repeat("0", 64)) {
		t.Fatalf("deleted balance did not read zero: %v %+v", err, zero)
	}
}
