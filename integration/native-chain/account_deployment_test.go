//go:build lithovm_chain_lab

package nativechain

import (
	"bytes"
	"errors"
	"fmt"
	"testing"

	storetypes "cosmossdk.io/store/types"
	"github.com/cosmos/cosmos-sdk/testutil"
	"github.com/ethereum/go-ethereum/common"
	"github.com/evmos/evmos/v20/x/evm/core/vm"
	"github.com/evmos/evmos/v20/x/evm/statedb"
)

func TestNativeAccountDeploymentBudgetBoundaryAndGenesisLifecycle(t *testing.T) {
	req := request(t)
	req.Bytecode = ptr(fixture(t, "../../sdk/contracts/standards/lep10015_native_account_candidate.lithic"))
	descriptors := fmt.Sprintf("%064x%064x", 32, 2)
	for _, address := range []string{"7e5f4552091a69125d5dfcb7b8c2659029395bdf", "2b5ad5c4795c026514f8317c7a215e218dccd6cf"} {
		descriptors += "000000000000000000000000" + address + fmt.Sprintf("%064x", 1)
	}
	req.Arguments = fmt.Sprintf("0x4c56414c02000207%04x%s01%064x", len(descriptors)/2, descriptors, 2)
	env := Environment{ChainID: 700780, Height: 1, Timestamp: 100, Nonce: 1}
	var exact uint64
	for step := 0; step < 3; step++ {
		budget := nativeExecutionGasCap
		if step == 1 {
			budget = exact
		}
		if step == 2 {
			budget = exact - 1
		}
		key := storetypes.NewKVStoreKey("account-budget")
		ctx := testutil.DefaultContext(key, storetypes.NewTransientStoreKey("account-budget-t"))
		db := statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
		f := frame(budget)
		result, err := ExecuteFrame(db, key, f, false, env, req)
		if step == 2 {
			if !errors.Is(err, vm.ErrOutOfGas) || f.Gas != 0 || len(db.Logs()) != 0 {
				t.Fatalf("budget exhaustion: %v gas=%d", err, f.Gas)
			}
			cache, _ := db.GetCacheContext()
			it := cache.KVStore(key).Iterator(nil, nil)
			hasState := it.Valid()
			it.Close()
			if hasState {
				t.Fatal("failed deployment retained state")
			}
			continue
		}
		if err != nil || !result.Success {
			t.Fatalf("account deploy: %v %+v", err, result)
		}
		used := budget - f.Gas
		if step == 0 {
			exact = used
			t.Logf("native deployment gas=%d / cap=%d", exact, nativeExecutionGasCap)
			if exact != 7_156_313 {
				t.Fatalf("keeper/Rust pricing replay differs: %d", exact)
			}
		} else if f.Gas != 0 {
			t.Fatalf("exact budget left %d", f.Gas)
		}
		if err := db.Commit(); err != nil {
			t.Fatal(err)
		}
		snapshot, err := ExportNativeGenesis(ctx.KVStore(key))
		if err != nil {
			t.Fatal(err)
		}
		destKey := storetypes.NewKVStoreKey("account-budget-restored")
		destCtx := testutil.DefaultContext(destKey, storetypes.NewTransientStoreKey("account-restored-t"))
		if err := ImportNativeGenesis(destCtx.KVStore(destKey), snapshot); err != nil {
			t.Fatal(err)
		}
		restored, err := ExportNativeGenesis(destCtx.KVStore(destKey))
		if err != nil || !bytes.Equal(restored, snapshot) {
			t.Fatalf("binary record export/import: %v", err)
		}
		address := returnedAddress(t, result)
		call := req
		call.Operation = "call"
		call.Bytecode = nil
		call.Contract = &address
		call.Function = ptr("threshold")
		call.Arguments = args()
		reload := statedb.New(destCtx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
		view, err := ExecuteFrame(reload, destKey, frame(nativeExecutionGasCap), false, env, call)
		if err != nil || !view.Success || view.Result != args(2) {
			t.Fatalf("restored account view: %v %+v", err, view)
		}
		if err := reload.Commit(); err != nil {
			t.Fatal(err)
		}
		after, err := ExportNativeGenesis(destCtx.KVStore(destKey))
		if err != nil || !bytes.Equal(after, snapshot) {
			t.Fatal("restored view mutated persisted state")
		}
	}
}
