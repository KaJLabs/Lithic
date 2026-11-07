//go:build lithovm_chain_lab && lithovm_bank_candidate

package nativechain

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"math/big"
	"strings"
	"testing"

	storetypes "cosmossdk.io/store/types"
	"github.com/cosmos/cosmos-sdk/testutil"
	sdk "github.com/cosmos/cosmos-sdk/types"
	"github.com/ethereum/go-ethereum/common"
	"github.com/evmos/evmos/v20/x/evm/statedb"
)

// Legacy fixtures deliberately contain no bank activity. The production
// keeper policy is tested separately against actual auth/bank keepers.
func (keeper) ValidateNativeBalanceAddress(sdk.Context, common.Address) error { return nil }

// Real SDK cache/journal, with small KV-backed account adapter. All reads use
// the supplied context so the test detects a bypass of live SDK gas metering.
type fundedKeeper struct {
	keeper
	key     storetypes.StoreKey
	guards  map[common.Address]int
	blocked common.Address
	fail    common.Address
	lateOOG bool
}

func accountKey(addr common.Address) []byte { return append([]byte("bank/"), addr[:]...) }
func (k *fundedKeeper) GetAccount(ctx sdk.Context, addr common.Address) *statedb.Account {
	raw := ctx.KVStore(k.key).Get(accountKey(addr))
	if raw == nil {
		return nil
	}
	var a statedb.Account
	if err := json.Unmarshal(raw, &a); err != nil {
		panic(err)
	}
	return &a
}
func (k *fundedKeeper) SetAccount(ctx sdk.Context, addr common.Address, a statedb.Account) error {
	raw, err := json.Marshal(a)
	if err == nil {
		ctx.KVStore(k.key).Set(accountKey(addr), raw)
	}
	return err
}
func (k *fundedKeeper) ValidateNativeBalanceAddress(ctx sdk.Context, addr common.Address) error {
	// A physical policy read must be charged even for cached EVM objects.
	ctx.KVStore(k.key).Get([]byte("policy"))
	k.guards[addr]++
	if addr == k.blocked {
		return errors.New("blocked bank account")
	}
	if addr == k.fail && k.guards[addr] >= 3 {
		if k.lateOOG {
			ctx.GasMeter().ConsumeGas(nativeExecutionGasCap, "late bank policy read")
		}
		return errors.New("late bank policy failure")
	}
	return nil
}
func (k *fundedKeeper) balance(ctx sdk.Context, addr common.Address) *big.Int {
	if a := k.GetAccount(ctx, addr); a != nil {
		return a.Balance
	}
	return new(big.Int)
}
func custodyRequest(t *testing.T) Request {
	req := request(t)
	req.Version = 2
	req.Bytecode = ptr(fixture(t, "testdata/custody.lithic"))
	req.GasLimit = nativeExecutionGasCap
	req.Arguments = "0x4c56414c01000104" + fmt.Sprintf("%064x", 9)
	return req
}
func custodyCall(req Request, vault common.Address, amount uint64, accept bool) Request {
	req.Operation = "call"
	req.Bytecode = nil
	req.Contract = ptr(strings.ToLower(vault.Hex()))
	req.Function = ptr("pay")
	flag := 0
	if accept {
		flag = 1
	}
	req.Arguments = fmt.Sprintf("0x4c56414c01000304%064x02%064x03%064x", 10, amount, flag)
	return req
}
func custodySetup(t *testing.T) (sdk.Context, *fundedKeeper, *statedb.StateDB, Request, common.Address) {
	t.Helper()
	key := storetypes.NewKVStoreKey("custody")
	ctx := testutil.DefaultContext(key, storetypes.NewTransientStoreKey("custody-t"))
	k := &fundedKeeper{key: key, guards: make(map[common.Address]int)}
	req := custodyRequest(t)
	// Discover the deterministic address without applying any external state.
	candidate, err := ExecuteCustody(req, func(string) ([]byte, error) { return nil, nil }, func([20]byte) ([32]byte, error) { return [32]byte{}, nil }, func(uint64) error { return nil })
	if err != nil || !candidate.Success {
		t.Fatal(err, candidate)
	}
	vault := common.HexToAddress(returnedAddress(t, candidate.Response))
	// Funding before native deployment must preserve the EVM account metadata.
	a := statedb.NewEmptyAccount()
	a.Balance = big.NewInt(100)
	a.Nonce = 17
	if err := k.SetAccount(ctx, vault, *a); err != nil {
		t.Fatal(err)
	}
	db := statedb.New(ctx, k, statedb.NewEmptyTxConfig(common.Hash{}))
	result, err := ExecuteCustodyFrame(db, key, frame(nativeExecutionGasCap), false, Environment{700777, 1, 2, 1}, req)
	if err != nil || !result.Success {
		t.Fatal("deploy", err, result)
	}
	if db.GetBalance(vault).Cmp(big.NewInt(100)) != 0 || db.GetNonce(vault) != 17 {
		t.Fatal("deployment lost prefunding or nonce")
	}
	if err := db.Commit(); err != nil {
		t.Fatal(err)
	}
	k.guards = make(map[common.Address]int)
	return ctx, k, statedb.New(ctx, k, statedb.NewEmptyTxConfig(common.Hash{})), req, vault
}

func TestCustodyFrameCommitAndEnclosingRevert(t *testing.T) {
	ctx, k, db, req, vault := custodySetup(t)
	to := common.HexToAddress("0x0a")
	env := Environment{700777, 1, 2, 1}
	// A prior pending ordinary EVM transfer is visible to the native call.
	donor := common.HexToAddress("0x0b")
	a := statedb.NewEmptyAccount()
	a.Balance = big.NewInt(20)
	k.SetAccount(ctx, donor, *a)
	db.SubBalance(donor, big.NewInt(20))
	db.AddBalance(vault, big.NewInt(20))
	outer := db.Snapshot()
	result, err := ExecuteCustodyFrame(db, k.key, frame(nativeExecutionGasCap), false, env, custodyCall(req, vault, 7, true))
	if err != nil || !result.Success {
		t.Fatal(err, result)
	}
	if db.GetBalance(vault).Cmp(big.NewInt(113)) != 0 || db.GetBalance(to).Cmp(big.NewInt(7)) != 0 {
		t.Fatal("wrong staged settlement")
	}
	if k.balance(ctx, vault).Cmp(big.NewInt(100)) != 0 || k.balance(ctx, to).Sign() != 0 {
		t.Fatal("bank changed before commit")
	}
	if len(db.Logs()) != 2 {
		t.Fatal("native and net-bank logs missing", len(db.Logs()))
	}
	if db.GetNonce(vault) != 17 {
		t.Fatal("nonce reset")
	}
	db.RevertToSnapshot(outer)
	if db.GetBalance(vault).Cmp(big.NewInt(120)) != 0 || db.GetBalance(to).Sign() != 0 || len(db.Logs()) != 0 {
		t.Fatal("enclosing revert leaked balances/logs")
	}
	result, err = ExecuteCustodyFrame(db, k.key, frame(nativeExecutionGasCap), false, env, custodyCall(req, vault, 7, true))
	if err != nil || !result.Success {
		t.Fatal(err, result)
	}
	if err := db.Commit(); err != nil {
		t.Fatal(err)
	}
	if k.balance(ctx, vault).Cmp(big.NewInt(113)) != 0 || k.balance(ctx, to).Cmp(big.NewInt(7)) != 0 || k.balance(ctx, donor).Sign() != 0 {
		t.Fatal("bank commit differs from journal")
	}
	total := new(big.Int).Add(k.balance(ctx, vault), k.balance(ctx, to))
	total.Add(total, k.balance(ctx, donor))
	if total.Cmp(big.NewInt(120)) != 0 {
		t.Fatal("supply changed", total)
	}
}

func TestCustodyFrameFailuresAreAtomicIncludingLateSettlementOOG(t *testing.T) {
	for _, fault := range []string{"revert", "unauthorized", "insufficient", "overflow", "blocked", "late-policy", "late-oog", "fuel"} {
		t.Run(fault, func(t *testing.T) {
			ctx, k, db, req, vault := custodySetup(t)
			to := common.HexToAddress("0x0a")
			amount := uint64(7)
			accept := fault != "revert"
			if fault == "insufficient" {
				amount = 101
			}
			if fault == "overflow" {
				a := statedb.NewEmptyAccount()
				a.Balance = new(big.Int).Sub(new(big.Int).Lsh(big.NewInt(1), 256), big.NewInt(1))
				k.SetAccount(ctx, to, *a)
			}
			if fault == "blocked" {
				k.blocked = to
			}
			if fault == "late-policy" || fault == "late-oog" {
				k.fail = vault
				k.lateOOG = fault == "late-oog"
			}
			req = custodyCall(req, vault, amount, accept)
			callFrame := frame(nativeExecutionGasCap)
			if fault == "unauthorized" {
				callFrame.CallerAddress = common.HexToAddress("0x0c")
			}
			if fault == "fuel" {
				callFrame = frame(1)
			}
			stateKey := []byte("lithovm/v1/contracts/" + strings.TrimPrefix(strings.ToLower(vault.Hex()), "0x"))
			before := append([]byte(nil), ctx.KVStore(k.key).Get(stateKey)...)
			bankBefore := new(big.Int).Set(k.balance(ctx, to))
			result, err := ExecuteCustodyFrame(db, k.key, callFrame, false, Environment{700777, 1, 2, 1}, req)
			if err == nil || result.Success {
				t.Fatal("failure accepted", err, result)
			}
			if fault == "late-policy" || fault == "late-oog" {
				if k.guards[vault] < 3 || k.guards[to] < 3 {
					t.Fatal("did not reach partial settlement", k.guards)
				}
			}
			if db.GetBalance(vault).Cmp(big.NewInt(100)) != 0 || db.GetBalance(to).Cmp(bankBefore) != 0 || len(db.Logs()) != 0 {
				t.Fatal("failed frame leaked journal effects")
			}
			cache, _ := db.GetCacheContext()
			if !bytes.Equal(cache.KVStore(k.key).Get(stateKey), before) {
				t.Fatal("failed frame leaked native state")
			}
			if err := db.Commit(); err != nil {
				t.Fatal(err)
			}
			if k.balance(ctx, vault).Cmp(big.NewInt(100)) != 0 || k.balance(ctx, to).Cmp(bankBefore) != 0 {
				t.Fatal("failed frame changed bank")
			}
		})
	}
}

func TestCustodyPhysicalReadsUseLiveGasAndUnsupportedFramesStayRejected(t *testing.T) {
	_, k, db, req, vault := custodySetup(t)
	req = custodyCall(req, vault, 7, true)
	f := frame(nativeExecutionGasCap)
	result, err := ExecuteCustodyFrame(db, k.key, f, false, Environment{700777, 1, 2, 1}, req)
	if err != nil || !result.Success {
		t.Fatal(err, result)
	}
	if k.guards[vault] != 3 || k.guards[common.HexToAddress("0x0a")] != 3 {
		t.Fatal("physical policy reads not repeated at settlement", k.guards)
	}
	if f.Gas >= nativeExecutionGasCap-2*candidateBankChangeGas {
		t.Fatal("bank reads/log/native work were not charged")
	}
	if _, err := ExecuteCustodyFrame(db, k.key, frame(nativeExecutionGasCap), true, Environment{700777, 1, 2, 1}, req); err == nil {
		t.Fatal("static frame accepted")
	}
	legacy := statedb.New(db.GetContext(), struct{ statedb.Keeper }{k}, statedb.NewEmptyTxConfig(common.Hash{}))
	if _, _, err := legacy.ReadNativeBalance(vault, storetypes.NewInfiniteGasMeter()); err == nil {
		t.Fatal("keeper without policy accepted")
	}
}

func TestCustodyBankReadPricingSurvivesEVMZeroKVGasContext(t *testing.T) {
	ctx, k, _, _, vault := custodySetup(t)
	var used uint64
	for _, zeroKV := range []bool{false, true} {
		c := ctx
		if zeroKV {
			c = c.WithKVGasConfig(storetypes.GasConfig{}).WithTransientKVGasConfig(storetypes.GasConfig{})
		}
		db := statedb.New(c, k, statedb.NewEmptyTxConfig(common.Hash{}))
		meter := storetypes.NewInfiniteGasMeter()
		balance, _, err := db.ReadNativeBalance(vault, meter)
		if err != nil || balance.Cmp(big.NewInt(100)) != 0 {
			t.Fatal(err, balance)
		}
		if meter.GasConsumed() == 0 {
			t.Fatal("bank read escaped live gas")
		}
		if !zeroKV {
			used = meter.GasConsumed()
		} else if meter.GasConsumed() != used {
			t.Fatal("EVM query context changed bank pricing", used, meter.GasConsumed())
		}
	}
}

func TestCustodyRejectsPendingSelfDestructAndInvalidUint256(t *testing.T) {
	ctx, k, db, _, vault := custodySetup(t)
	db.Suicide(vault)
	if _, _, err := db.ReadNativeBalance(vault, storetypes.NewInfiniteGasMeter()); err == nil {
		t.Fatal("pending account deletion accepted")
	}
	for _, amount := range []*big.Int{big.NewInt(-1), new(big.Int).Lsh(big.NewInt(1), 256)} {
		a := statedb.NewEmptyAccount()
		a.Balance = amount
		k.SetAccount(ctx, vault, *a)
		db = statedb.New(ctx, k, statedb.NewEmptyTxConfig(common.Hash{}))
		if _, _, err := db.ReadNativeBalance(vault, storetypes.NewInfiniteGasMeter()); err == nil {
			t.Fatal("invalid bank uint256 accepted", amount)
		}
	}
}
