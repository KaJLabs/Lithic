//go:build lithovm_chain_lab && lithovm_bank_candidate

package nativechain

import (
	"encoding/json"
	"errors"
	"math/big"

	storetypes "cosmossdk.io/store/types"
	"github.com/ethereum/go-ethereum/common"
	ethtypes "github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/evmos/evmos/v20/x/evm/core/vm"
	"github.com/evmos/evmos/v20/x/evm/statedb"
)

const NativeCustodyCandidateEnabled = true

// Disabled candidate bookkeeping references existing EVM CALL value/new-account
// prices. Actual physical SDK reads are charged separately and live. Final bank
// economics require Foundation review; these constants do not alter v1/v2.
const candidateBankChangeGas uint64 = 9000
const candidateBankNewAccountGas uint64 = 25000

type custodyLedger interface {
	ReadNativeBalance(common.Address, storetypes.GasMeter) (*big.Int, bool, error)
	ApplyNativeBalance(common.Address, *big.Int, *big.Int, storetypes.GasMeter) error
}

type sdkFrameMeter struct {
	storetypes.GasMeter
	live *frameMeter
}

func (m *sdkFrameMeter) ConsumeGas(amount storetypes.Gas, descriptor string) {
	if m.live.charge(uint64(amount)) != nil {
		panic(storetypes.ErrorOutOfGas{Descriptor: descriptor})
	}
	m.GasMeter.ConsumeGas(amount, descriptor)
}

func executeSelectedProfile(db *statedb.StateDB, key storetypes.StoreKey, frame *vm.Contract, readOnly bool, env Environment, request Request) (Response, error) {
	return ExecuteCustodyFrame(db, key, frame, readOnly, env, request)
}

// Gateway msg.value remains zero: native accounts are funded through ordinary
// bank/EVM transfers to their address. There is no alternate native-balance KV.
func ExecuteCustodyFrame(db *statedb.StateDB, key storetypes.StoreKey, frame *vm.Contract, readOnly bool, env Environment, request Request) (Response, error) {
	ledger, ok := any(db).(custodyLedger)
	if !ok {
		return Response{}, errors.New("chain StateDB does not support custody policy/metering")
	}
	var result BankResponse
	originals := make(map[[20]byte][32]byte)
	var sdkMeter *sdkFrameMeter
	effects := &frameEffects{}
	effects.bindMeter = func(meter *frameMeter) {
		sdkMeter = &sdkFrameMeter{GasMeter: storetypes.NewInfiniteGasMeter(), live: meter}
	}
	effects.execute = func(req Request, read Reader, charge Charger) (Response, error) {
		req.Version = 2
		var err error
		result, err = ExecuteCustody(req, read, func(address [20]byte) ([32]byte, error) {
			var word [32]byte
			if len(originals) >= 128 {
				if _, known := originals[address]; !known {
					return word, errors.New("custody footprint exceeds limit")
				}
			}
			value, _, err := ledger.ReadNativeBalance(common.Address(address), sdkMeter)
			if err != nil {
				return word, err
			}
			value.FillBytes(word[:])
			originals[address] = word
			return word, nil
		}, charge)
		return result.Response, err
	}
	effects.prepare = func(meter *frameMeter) (func() error, []*ethtypes.Log, error) {
		updates, err := result.DecodedBalances()
		if err != nil {
			return nil, nil, err
		}
		for _, update := range updates {
			original, known := originals[update.address]
			if !known || new(big.Int).SetBytes(original[:]).Cmp(update.before) != 0 {
				return nil, nil, errors.New("custody before-balance was not independently read")
			}
			current, exists, err := ledger.ReadNativeBalance(common.Address(update.address), sdkMeter)
			if err != nil {
				return nil, nil, err
			}
			if current.Cmp(update.before) != 0 {
				return nil, nil, errors.New("custody balance changed during native execution")
			}
			cost := candidateBankChangeGas
			if !exists {
				cost = gasSum(cost, candidateBankNewAccountGas)
			}
			if err := meter.charge(cost); err != nil {
				return nil, nil, err
			}
		}
		apply := func() error {
			for _, update := range updates {
				if err := ledger.ApplyNativeBalance(common.Address(update.address), update.before, update.after, sdkMeter); err != nil {
					return err
				}
			}
			return nil
		}
		if len(updates) == 0 {
			return apply, nil, nil
		}
		data, err := json.Marshal(struct {
			Version int             `json:"version"`
			Changes []BalanceChange `json:"changes"`
		}{1, result.Balances})
		if err != nil {
			return nil, nil, err
		}
		logs := []*ethtypes.Log{{Address: LabAddress, Topics: []common.Hash{crypto.Keccak256Hash([]byte("LithoNativeBalancesV1(bytes)"))}, Data: data}}
		return apply, logs, nil
	}
	return executeFrameProfile(db, key, frame, readOnly, env, request, effects)
}
