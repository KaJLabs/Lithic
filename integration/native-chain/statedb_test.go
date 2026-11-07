//go:build lithovm_chain_lab

package nativechain

import (
	storetypes "cosmossdk.io/store/types"
	"encoding/hex"
	"fmt"
	"github.com/cosmos/cosmos-sdk/testutil"
	sdk "github.com/cosmos/cosmos-sdk/types"
	"github.com/ethereum/go-ethereum/common"
	ethtypes "github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/ethereum/go-ethereum/params"
	"github.com/evmos/evmos/v20/x/evm/core/vm"
	"github.com/evmos/evmos/v20/x/evm/statedb"
	"math/big"
	"strings"
	"testing"
)

// Only EVM account backing is minimal; SDK KV cache and StateDB journals are real.
type keeper struct{}

func TestFrameGasAboveNativeCapPreservesSuccessAndUnusedGas(t *testing.T) {
	var used uint64
	for _, budget := range []uint64{1000000, 9999999, 10000000, 10000001, 15000000, 30000000} {
		t.Run(fmt.Sprint(budget), func(t *testing.T) {
			key := storetypes.NewKVStoreKey("gas-cap")
			ctx := testutil.DefaultContext(key, storetypes.NewTransientStoreKey("gas-cap-transient"))
			db := statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
			callFrame := frame(budget)
			result, err := ExecuteFrame(db, key, callFrame, false,
				Environment{ChainID: 700777, Height: 1, Timestamp: 2, Nonce: 1}, request(t))
			if err != nil || !result.Success {
				t.Fatalf("budget %d: error=%v success=%v remaining=%d", budget, err, result.Success, callFrame.Gas)
			}
			consumed := budget - callFrame.Gas
			if used == 0 {
				used = consumed
			}
			if consumed != used || callFrame.Gas == 0 {
				t.Fatalf("gas must depend on work, not budget: used=%d expected=%d remaining=%d", consumed, used, callFrame.Gas)
			}
		})
	}
}

func (keeper) GetAccount(sdk.Context, common.Address) *statedb.Account {
	return statedb.NewEmptyAccount()
}

func TestRealStateDBFactoryChildAtomicRollback(t *testing.T) {
	key := storetypes.NewKVStoreKey("lithovm-factory-lab")
	ctx := testutil.DefaultContext(key, storetypes.NewTransientStoreKey("transient-factory"))
	db := statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
	env := Environment{ChainID: 700777, Height: 1, Timestamp: 2, Nonce: 1}
	deployCode := func(source string, nonce uint64) string {
		req := request(t)
		req.Bytecode = ptr(fixture(t, source))
		req.Function = nil
		req.Arguments = args()
		if source == "testdata/child.lithic" {
			req.Function = ptr("initialize")
			req.Arguments = args(0)
		}
		env.Nonce = nonce
		result, err := ExecuteFrame(db, key, frame(1000000), false, env, req)
		if err != nil || !result.Success {
			t.Fatalf("deploy %s: %v %+v", source, err, result)
		}
		return returnedAddress(t, result)
	}
	template := deployCode("testdata/child.lithic", 1)
	factory := deployCode("testdata/factory.lithic", 2)
	selector := crypto.Keccak256Hash([]byte("initialize(u64)"))
	templateBytes, _ := hex.DecodeString(strings.TrimPrefix(template, "0x"))
	makeArgs := func(accept bool) string {
		flag := 0
		if accept {
			flag = 1
		}
		return fmt.Sprintf("0x4c56414c01000504%064x05%064x05%x01%064x03%064x", templateBytes, uint64(1), selector[:], uint64(42), flag)
	}
	req := request(t)
	req.Operation = "call"
	req.Bytecode = nil
	req.Contract = &factory
	req.Function = ptr("create")
	req.Arguments = makeArgs(false)
	outer := db.Snapshot()
	failed, err := ExecuteFrame(db, key, frame(1000000), false, env, req)
	if err == nil || failed.Success || len(failed.Writes) != 0 {
		t.Fatal("parent failure leaked child", err, failed)
	}
	db.RevertToSnapshot(outer)
	req.Arguments = makeArgs(true)
	outer = db.Snapshot()
	success, err := ExecuteFrame(db, key, frame(1000000), false, env, req)
	if err != nil || !success.Success || len(success.Deployments) != 1 || len(success.Events) != 2 {
		t.Fatal("creation failed", err, success)
	}
	child := returnedAddress(t, success)
	childKey := []byte("lithovm/v1/contracts/" + strings.TrimPrefix(child, "0x"))
	cache, _ := db.GetCacheContext()
	if !cache.KVStore(key).Has(childKey) {
		t.Fatal("missing staged child")
	}
	db.RevertToSnapshot(outer)
	cache, _ = db.GetCacheContext()
	if cache.KVStore(key).Has(childKey) {
		t.Fatal("outer revert retained child")
	}
	if len(db.Logs()) != 2 {
		t.Fatalf("expected two template/factory envelope logs after revert, got %d", len(db.Logs()))
	}
	success, err = ExecuteFrame(db, key, frame(1000000), false, env, req)
	if err != nil || !success.Success {
		t.Fatal("retry failed", err)
	}
	if err := db.Commit(); err != nil {
		t.Fatal(err)
	}
	if !ctx.KVStore(key).Has(childKey) {
		t.Fatal("child not durable after commit")
	}
	db = statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
	get := request(t)
	get.Operation = "call"
	get.Bytecode = nil
	get.Contract = &child
	get.Function = ptr("get")
	get.Arguments = args()
	observed, err := ExecuteFrame(db, key, frame(1000000), false, env, get)
	if err != nil || !strings.HasSuffix(observed.Result, fmt.Sprintf("%064x", 42)) {
		t.Fatal("child reload", err, observed)
	}
}
func (keeper) GetState(sdk.Context, common.Address, common.Hash) common.Hash { return common.Hash{} }

func TestPerKeyMapReadYourWritesAcrossSynchronousFrames(t *testing.T) {
	key := storetypes.NewKVStoreKey("map-relay")
	ctx := testutil.DefaultContext(key, storetypes.NewTransientStoreKey("map-relay-t"))
	env := Environment{ChainID: 700777, Height: 1, Timestamp: 2, Nonce: 1}
	db := statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
	deploy := func(source string, nonce uint64, initialize bool) string {
		req := request(t)
		req.Bytecode = ptr(fixture(t, source))
		req.Function, req.Arguments = nil, args()
		if initialize {
			req.Function = ptr("initialize")
		}
		env.Nonce = nonce
		result, err := ExecuteFrame(db, key, frame(10_000_000), false, env, req)
		if err != nil || !result.Success {
			t.Fatalf("deploy %s: %v %+v", source, err, result)
		}
		return returnedAddress(t, result)
	}
	child := deploy("testdata/storage_gas.lithic", 1, true)
	relay := deploy("testdata/map_relay.lithic", 2, false)
	if err := db.Commit(); err != nil {
		t.Fatal(err)
	}
	writeSelector := crypto.Keccak256Hash([]byte("write(address,u256)"))
	readSelector := crypto.Keccak256Hash([]byte("read(address)"))
	targetWord := strings.Repeat("0", 24) + strings.TrimPrefix(child, "0x")
	mapKey := "lithovm/v2/maps/" + strings.TrimPrefix(child, "0x") + "/balances/" + fmt.Sprintf("%064x", 7)
	req := request(t)
	req.Operation, req.Bytecode, req.Contract, req.Function = "call", nil, &relay, ptr("run")
	callArgs := func(accept bool) string {
		flag := 0
		if accept {
			flag = 1
		}
		return fmt.Sprintf("0x4c56414c01000604%s05%x05%x04%064x02%064x03%064x", targetWord, writeSelector[:], readSelector[:], 7, 42, flag)
	}
	db = statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
	outer := db.Snapshot()
	req.Arguments = callArgs(false)
	failed, err := ExecuteFrame(db, key, frame(10_000_000), false, env, req)
	if err == nil || failed.Success || len(failed.Writes) != 0 {
		t.Fatalf("failed relay leaked effects: %v %+v", err, failed)
	}
	db.RevertToSnapshot(outer)
	if ctx.KVStore(key).Has([]byte(mapKey)) {
		t.Fatal("failed relay persisted child map word")
	}
	db = statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
	req.Arguments = callArgs(true)
	success, err := ExecuteFrame(db, key, frame(10_000_000), false, env, req)
	if err != nil || !success.Success || !strings.HasSuffix(success.Result, fmt.Sprintf("%064x", 42)) {
		t.Fatalf("second child frame missed staged map value: %v %+v", err, success)
	}
	if err := db.Commit(); err != nil {
		t.Fatal(err)
	}
	if value := ctx.KVStore(key).Get([]byte(mapKey)); len(value) != 32 || value[31] != 42 {
		t.Fatalf("successful relay map value not durable: %x", value)
	}
}
func (keeper) GetCode(sdk.Context, common.Hash) []byte                                         { return nil }
func (keeper) ForEachStorage(sdk.Context, common.Address, func(common.Hash, common.Hash) bool) {}
func (keeper) SetAccount(sdk.Context, common.Address, statedb.Account) error                   { return nil }
func (keeper) SetState(sdk.Context, common.Address, common.Hash, []byte)                       {}
func (keeper) SetCode(sdk.Context, []byte, []byte)                                             {}
func (keeper) DeleteAccount(sdk.Context, common.Address) error                                 { return nil }
func frame(gas uint64) *vm.Contract {
	return vm.NewContract(vm.AccountRef(common.HexToAddress("0x0000000000000000000000000000000000000009")), vm.AccountRef(LabAddress), big.NewInt(0), gas)
}

func TestEphemeralEVMPrecompileCallAndOuterRevert(t *testing.T) {
	key := storetypes.NewKVStoreKey("lithovm-evm-call-lab")
	ctx := testutil.DefaultContext(key, storetypes.NewTransientStoreKey("transient-evm-call"))
	db := statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
	evm := vm.NewEVM(vm.BlockContext{
		CanTransfer: func(vm.StateDB, common.Address, *big.Int) bool { return true },
		Transfer:    func(vm.StateDB, common.Address, common.Address, *big.Int) {},
		BlockNumber: big.NewInt(11), Time: big.NewInt(22),
	}, vm.TxContext{Origin: common.HexToAddress("0x0000000000000000000000000000000000000009")}, db, params.AllEthashProtocolChanges, vm.Config{})
	message := ethtypes.NewMessage(evm.TxContext.Origin, &LabAddress, 1, big.NewInt(0), 1000000, big.NewInt(0), big.NewInt(0), big.NewInt(0), nil, nil, false)
	if err := RegisterLabForMessage(evm, key, message, big.NewInt(700777), true); err != nil {
		t.Fatal(err)
	}
	if err := RegisterLabForMessage(evm, key, message, big.NewInt(0), true); err == nil {
		t.Fatal("accepted zero chain ID")
	}
	fake := ethtypes.NewMessage(evm.TxContext.Origin, &LabAddress, 1, big.NewInt(0), 1000000, big.NewInt(0), big.NewInt(0), big.NewInt(0), nil, nil, true)
	if err := RegisterLabForMessage(evm, key, fake, big.NewInt(700777), true); err == nil {
		t.Fatal("simulation was allowed to commit")
	}
	if err := RegisterLabForMessage(evm, key, fake, big.NewInt(700777), false); err != nil {
		t.Fatalf("discard-only simulation rejected: %v", err)
	}
	if p, ok := evm.Precompile(LabAddress); !ok || !p.(LabPrecompile).Transaction.Simulated {
		t.Fatal("simulation mode was not carried into gateway context")
	}
	if err := RegisterLabForMessage(evm, key, message, new(big.Int).Lsh(big.NewInt(1), 64), true); err == nil {
		t.Fatal("accepted overflowing chain ID")
	}
	wrongMessage := ethtypes.NewMessage(common.HexToAddress("0x0a"), &LabAddress, 1, big.NewInt(0), 1000000, big.NewInt(0), big.NewInt(0), big.NewInt(0), nil, nil, false)
	if err := RegisterLabForMessage(evm, key, wrongMessage, big.NewInt(700777), true); err == nil {
		t.Fatal("accepted message sender mismatch")
	}
	if err := RegisterLabForMessage(evm, key, message, big.NewInt(700777), true); err != nil {
		t.Fatal(err)
	}
	precompile, ok := evm.Precompile(LabAddress)
	if !ok || precompile.RequiredGas([]byte{1, 2, 3}) != 512 {
		t.Fatal("gateway decode gas not prepaid")
	}
	code, err := hex.DecodeString(strings.TrimPrefix(fixture(t, "testdata/counter.lithic"), "0x"))
	if err != nil {
		t.Fatal(err)
	}
	selector := crypto.Keccak256Hash([]byte("initialize(u64)"))
	arguments, err := hex.DecodeString(strings.TrimPrefix(args(7), "0x"))
	if err != nil {
		t.Fatal(err)
	}
	payload, err := EncodeGatewayDeploy(code, selector, arguments)
	if err != nil {
		t.Fatal(err)
	}
	caller := vm.AccountRef(common.HexToAddress("0x0000000000000000000000000000000000000009"))
	wrapper := vm.AccountRef(common.HexToAddress("0x000000000000000000000000000000000000000a"))
	for _, budget := range []uint64{1000, 30_000_000} {
		if _, left, err := evm.Call(caller, LabAddress, []byte{1, 2, 3, 4}, budget, big.NewInt(0)); err == nil || left != 0 {
			t.Fatalf("malformed gateway input did not consume EVM frame gas at %d: %v %d", budget, err, left)
		}
	}
	if _, left, err := evm.Call(wrapper, LabAddress, payload, 1000000, big.NewInt(0)); err != vm.ErrExecutionReverted || left == 0 {
		t.Fatalf("wrapper deployment was not rejected: %v %d", err, left)
	}
	if len(db.Logs()) != 0 {
		t.Fatal("wrapper deployment emitted native log")
	}
	outer := db.Snapshot()
	output, left, err := evm.Call(caller, LabAddress, payload, 1000000, big.NewInt(0))
	if err != nil || left >= 1000000 {
		t.Fatalf("EVM precompile call: %v gas=%d", err, left)
	}
	baselineUsed := uint64(1000000) - left
	for _, budget := range []uint64{9999999, 10000000, 10000001, 15000000, 30000000} {
		// Each budget is a separate transaction, including the StateDB call counter.
		probeCtx := testutil.DefaultContext(key, storetypes.NewTransientStoreKey("gas-probe"))
		probeDB := statedb.New(probeCtx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
		probe := vm.NewEVM(evm.Context, evm.TxContext, probeDB, params.AllEthashProtocolChanges, vm.Config{})
		if err := RegisterLabForMessage(probe, key, message, big.NewInt(700777), true); err != nil {
			t.Fatal(err)
		}
		observed, remaining, callErr := probe.Call(caller, LabAddress, payload, budget, big.NewInt(0))
		if callErr != nil || string(observed) != string(output) || budget-remaining != baselineUsed {
			t.Fatalf("EVM gas monotonicity budget=%d remaining=%d error=%v", budget, remaining, callErr)
		}
	}
	nativeResult, err := DecodeGatewayResult(output)
	if err != nil {
		t.Fatal(err)
	}
	result := Response{Success: true, Result: "0x" + hex.EncodeToString(nativeResult)}
	address := returnedAddress(t, result)
	stateKey := []byte("lithovm/v1/contracts/" + strings.TrimPrefix(address, "0x"))
	cache, _ := db.GetCacheContext()
	if !cache.KVStore(key).Has(stateKey) || len(db.Logs()) != 1 {
		t.Fatal("EVM call did not stage native state and log")
	}
	if !strings.Contains(string(db.Logs()[0].Data), "0000000000000000000000000000000000000009") {
		t.Fatal("native deployer was not the immediate EVM caller")
	}
	getSelector := crypto.Keccak256Hash([]byte("get()"))
	getArguments, err := hex.DecodeString(strings.TrimPrefix(args(), "0x"))
	if err != nil {
		t.Fatal(err)
	}
	getPayload, err := EncodeGatewayCall(common.HexToAddress(address), getSelector, getArguments)
	if err != nil {
		t.Fatal(err)
	}
	unknownSelector := crypto.Keccak256Hash([]byte("missing()"))
	unknownPayload, err := EncodeGatewayCall(common.HexToAddress(address), unknownSelector, getArguments)
	if err != nil {
		t.Fatal(err)
	}
	if _, left, err := evm.Call(caller, LabAddress, unknownPayload, 1000000, big.NewInt(0)); err != vm.ErrExecutionReverted || left == 0 {
		t.Fatalf("unknown native selector failure gas: %v %d", err, left)
	}
	getOutput, _, err := evm.Call(caller, LabAddress, getPayload, 1000000, big.NewInt(0))
	if err != nil {
		t.Fatal(err)
	}
	getResult, err := DecodeGatewayResult(getOutput)
	if err != nil || !strings.HasSuffix(hex.EncodeToString(getResult), "0007") {
		t.Fatalf("native gateway call: %v %x", err, getResult)
	}
	if _, _, err := evm.Call(wrapper, LabAddress, getPayload, 1000000, big.NewInt(0)); err != nil {
		t.Fatalf("wrapper call to existing native contract should remain allowed: %v", err)
	}
	RegisterLabPrecompile(evm, key, GatewayTransaction{Origin: wrapper.Address(), ChainID: 700777, Nonce: 1})
	if _, left, err := evm.Call(caller, LabAddress, getPayload, 1000000, big.NewInt(0)); err == nil || left != 0 {
		t.Fatalf("mismatched authenticated transaction context accepted: %v %d", err, left)
	}
	if len(db.Logs()) != 1 {
		t.Fatal("rejected context changed native logs")
	}
	db.RevertToSnapshot(outer)
	cache, _ = db.GetCacheContext()
	if cache.KVStore(key).Has(stateKey) || len(db.Logs()) != 0 {
		t.Fatal("EVM outer revert retained native effects")
	}
}

func TestRealStateDBOuterRollbackCommitAndGuards(t *testing.T) {
	key := storetypes.NewKVStoreKey("lithovm-lab")
	ctx := testutil.DefaultContext(key, storetypes.NewTransientStoreKey("transient"))
	db := statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
	env := Environment{ChainID: 700777, Height: 1, Timestamp: 2, Nonce: 1}
	req := request(t)
	// Native success must not survive an enclosing EVM snapshot revert.
	outer := db.Snapshot()
	response, err := ExecuteFrame(db, key, frame(1000000), false, env, req)
	if err != nil {
		t.Fatal(err)
	}
	address := returnedAddress(t, response)
	stateKey := []byte("lithovm/v1/contracts/" + strings.TrimPrefix(address, "0x"))
	cache, _ := db.GetCacheContext()
	if !cache.KVStore(key).Has(stateKey) || len(db.Logs()) != 1 {
		t.Fatal("missing staged state/log")
	}
	if ctx.KVStore(key).Has(stateKey) {
		t.Fatal("premature durable state")
	}
	db.RevertToSnapshot(outer)
	cache, _ = db.GetCacheContext()
	if cache.KVStore(key).Has(stateKey) || len(db.Logs()) != 0 {
		t.Fatal("outer rollback leaked state/log")
	}
	response, err = ExecuteFrame(db, key, frame(1000000), false, env, req)
	if err != nil || !response.Success {
		t.Fatal("retry", err)
	}
	if err := db.Commit(); err != nil {
		t.Fatal(err)
	}
	if !ctx.KVStore(key).Has(stateKey) {
		t.Fatal("missing committed state")
	}
	// Reload in a fresh StateDB and read persisted Rust storage.
	db = statedb.New(ctx, keeper{}, statedb.NewEmptyTxConfig(common.Hash{}))
	req.Operation = "call"
	req.Bytecode = nil
	req.Contract = &address
	req.Function = ptr("get")
	req.Arguments = args()
	read, err := ExecuteFrame(db, key, frame(1000000), false, env, req)
	if err != nil || !strings.HasSuffix(read.Result, "0007") {
		t.Fatal("reload", err, read)
	}
	if _, err := ExecuteFrame(db, key, frame(1000000), true, env, req); err == nil {
		t.Fatal("static accepted")
	}
	paid := vm.NewContract(vm.AccountRef(common.Address{}), vm.AccountRef(LabAddress), big.NewInt(1), 1000000)
	if _, err := ExecuteFrame(db, key, paid, false, env, req); err == nil {
		t.Fatal("payable accepted")
	}
	delegate := vm.NewContract(vm.AccountRef(common.Address{}), vm.AccountRef(common.HexToAddress("0x10")), big.NewInt(0), 1000000)
	if _, err := ExecuteFrame(db, key, delegate, false, env, req); err == nil {
		t.Fatal("delegate accepted")
	}
	before := string(ctx.KVStore(key).Get(stateKey))
	req.Function = ptr("set")
	req.Arguments = args(99, 1)
	if _, err := ExecuteFrame(db, key, frame(1), false, env, req); err == nil {
		t.Fatal("OOG accepted")
	}
	if string(ctx.KVStore(key).Get(stateKey)) != before {
		t.Fatal("OOG changed committed state")
	}
}
