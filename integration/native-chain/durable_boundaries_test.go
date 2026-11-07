//go:build lithovm_chain_lab && linux

package nativechain

import (
	"bytes"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"cosmossdk.io/log"
	"cosmossdk.io/store"
	"cosmossdk.io/store/metrics"
	storetypes "cosmossdk.io/store/types"
	cmttypes "github.com/cometbft/cometbft/proto/tendermint/types"
	dbm "github.com/cosmos/cosmos-db"
	sdk "github.com/cosmos/cosmos-sdk/types"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/evmos/evmos/v20/x/evm/core/vm"
	"github.com/evmos/evmos/v20/x/evm/statedb"
)

func compileSizedCode(t *testing.T, functions, tailBytes int) []byte {
	t.Helper()
	var source strings.Builder
	source.WriteString("contract Sized { pub fn initialize() -> bool { return true; } ")
	for i := 0; i < functions; i++ {
		name := fmt.Sprintf("pad%04d_%s", i, strings.Repeat("x", 80))
		if i == functions-1 {
			name += strings.Repeat("y", tailBytes)
		}
		fmt.Fprintf(&source, "pub fn %s() -> u64 { return %d; } ", name, i)
	}
	source.WriteString("}")
	path := filepath.Join(t.TempDir(), "sized.lithic")
	if err := os.WriteFile(path, []byte(source.String()), 0o600); err != nil {
		t.Fatal(err)
	}
	out, err := exec.Command("../../target/debug/lithc", "--emit", "lithovm", path).CombinedOutput()
	if err != nil {
		t.Fatalf("compile %d functions: %v: %s", functions, err, out)
	}
	var artifact struct {
		Bytecode string `json:"bytecode"`
	}
	if err := json.Unmarshal(out, &artifact); err != nil {
		t.Fatal(err)
	}
	code, err := hex.DecodeString(strings.TrimPrefix(artifact.Bytecode, "0x"))
	if err != nil {
		t.Fatal(err)
	}
	return code
}

func codeAtSize(t *testing.T, target int) []byte {
	t.Helper()
	low, high := 0, 1023
	for low < high {
		mid := (low + high + 1) / 2
		if len(compileSizedCode(t, mid, 0)) <= target {
			low = mid
		} else {
			high = mid - 1
		}
	}
	base := compileSizedCode(t, low, 0)
	if low == 0 || len(base) > target || target-len(base) > 160 {
		t.Fatalf("cannot size compiler artifact to %d bytes: functions=%d base=%d", target, low, len(base))
	}
	code := compileSizedCode(t, low, target-len(base))
	if len(code) != target {
		t.Fatalf("compiler code size = %d, want %d", len(code), target)
	}
	return code
}

func TestDurableDeploymentAndLogBoundariesCandidate(t *testing.T) {
	if os.Getenv("LITHOVM_BOUNDARY_BENCH") != "1" {
		t.Skip("set LITHOVM_BOUNDARY_BENCH=1 for isolated durable boundary measurements")
	}
	dir := t.TempDir()
	physical, err := dbm.NewDB("boundary", dbm.GoLevelDBBackend, dir)
	if err != nil {
		t.Fatal(err)
	}
	defer physical.Close()
	key := storetypes.NewKVStoreKey("lithovm")
	cms := store.NewCommitMultiStore(physical, log.NewNopLogger(), metrics.NewNoOpMetrics())
	cms.MountStoreWithDB(key, storetypes.StoreTypeIAVL, physical)
	if err := cms.LoadLatestVersion(); err != nil {
		t.Fatal(err)
	}
	ctx := sdk.NewContext(cms, cmttypes.Header{}, false, log.NewNopLogger())
	env := Environment{ChainID: 700777, Height: 1, Timestamp: 2, Nonce: 1}
	selector := crypto.Keccak256Hash([]byte("initialize()"))
	var selected [32]byte
	copy(selected[:], selector[:])
	for _, size := range []int{4096, 16384, 32768, 65536, 65537} {
		code := codeAtSize(t, size)
		_, admissionErr := EncodeGatewayDeploy(code, selected, []byte{0x4c, 0x56, 0x41, 0x4c, 1, 0, 0})
		if size == 65537 {
			if admissionErr == nil {
				t.Fatal("gateway accepted 64-KiB + 1 byte")
			}
			t.Logf("BOUNDARY deploy_code_bytes=%d gateway=reject", size)
			continue
		}
		if admissionErr != nil {
			t.Fatalf("gateway rejected %d bytes: %v", size, admissionErr)
		}
		req := Request{Version: 1, Operation: "deploy", Bytecode: ptr("0x" + hex.EncodeToString(code)),
			Function: ptr("initialize"), Arguments: "0x4c56414c010000", Nonce: uint64(size), GasLimit: 10_000_000}
		before, _ := nativeStateSize(t, ctx, key)
		frame := frame(10_000_000)
		db := statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
		start := time.Now()
		result, execErr := ExecuteFrame(db, key, frame, false, env, req)
		execution := time.Since(start)
		gas := uint64(10_000_000) - frame.Gas
		if execErr == nil && result.Success {
			if err := db.Commit(); err != nil {
				t.Fatal(err)
			}
			commitStart := time.Now()
			cms.Commit()
			t.Logf("BOUNDARY deploy_code_bytes=%d gateway=accept execution=success gas=%d execute_ns=%d commit_ns=%d", size, gas, execution.Nanoseconds(), time.Since(commitStart).Nanoseconds())
		} else {
			after, _ := nativeStateSize(t, ctx, key)
			if before != after || len(db.Logs()) != 0 {
				t.Fatalf("failed deployment leaked state/logs: size=%d before=%d after=%d logs=%d", size, before, after, len(db.Logs()))
			}
			if !errors.Is(execErr, vm.ErrOutOfGas) {
				t.Fatalf("deployment %d failed unexpectedly: %v %+v", size, execErr, result)
			}
			t.Logf("BOUNDARY deploy_code_bytes=%d gateway=accept execution=out_of_gas gas=%d execute_ns=%d", size, gas, execution.Nanoseconds())
		}
	}

	code := fixture(t, "testdata/log_envelope.lithic")
	req := Request{Version: 1, Operation: "deploy", Bytecode: &code, Function: ptr("initialize"), Arguments: "0x4c56414c010000", GasLimit: 10_000_000}
	db := statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
	deployed, err := ExecuteFrame(db, key, frame(10_000_000), false, env, req)
	if err != nil || !deployed.Success {
		t.Fatalf("log contract deploy: %v %+v", err, deployed)
	}
	if err := db.Commit(); err != nil {
		t.Fatal(err)
	}
	cms.Commit()
	contract := returnedAddress(t, deployed)
	req.Operation, req.Bytecode, req.Contract, req.Function = "call", nil, &contract, ptr("note_many")
	for _, testcase := range []struct{ size, count int }{{1, 1}, {4096, 1}, {4096, 2}, {4096, 15}, {4096, 16}} {
		text := bytes.Repeat([]byte{'x'}, testcase.size)
		req.Arguments = fmt.Sprintf("0x4c56414c01000206%04x%s01%064x", len(text), hex.EncodeToString(text), testcase.count)
		frame := frame(10_000_000)
		db := statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
		start := time.Now()
		result, callErr := ExecuteFrame(db, key, frame, false, env, req)
		elapsed := time.Since(start)
		if testcase.count == 16 {
			if !errors.Is(callErr, vm.ErrExecutionReverted) || result.Success || len(db.Logs()) != 0 || !strings.Contains(result.Message, "dynamic event output exceeds byte limit") {
				t.Fatalf("over-envelope log call leaked effects: %v %+v logs=%d", callErr, result, len(db.Logs()))
			}
			t.Logf("BOUNDARY log_payload_bytes=%d events=%d outcome=revert gas=%d execute_ns=%d", testcase.size, testcase.count, 10_000_000-frame.Gas, elapsed.Nanoseconds())
			continue
		}
		if callErr != nil || !result.Success || len(result.Events) != testcase.count {
			t.Fatalf("log payload=%d events=%d: %v %+v", testcase.size, testcase.count, callErr, result)
		}
		if err := db.Commit(); err != nil {
			t.Fatal(err)
		}
		commitStart := time.Now()
		cms.Commit()
		t.Logf("BOUNDARY log_payload_bytes=%d events=%d outcome=success gas=%d execute_ns=%d commit_ns=%d", testcase.size, testcase.count, 10_000_000-frame.Gas, elapsed.Nanoseconds(), time.Since(commitStart).Nanoseconds())
	}
}
