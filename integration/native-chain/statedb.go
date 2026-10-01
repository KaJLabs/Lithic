//go:build lithovm_chain_lab

package nativechain

import (
	"bytes"
	"encoding/json"
	"errors"
	"sort"
	"strconv"
	"strings"

	storetypes "cosmossdk.io/store/types"
	"github.com/ethereum/go-ethereum/common"
	ethtypes "github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/evmos/evmos/v20/x/evm/core/vm"
	"github.com/evmos/evmos/v20/x/evm/statedb"
)

// LabAddress is never registered with the EVM precompile map.
var LabAddress = common.HexToAddress("0x000000000000000000000000000000000000f000")

type Environment struct{ ChainID, Height, Timestamp, Nonce uint64 }

// Must match the Rust FFI execution safety limit. Successful native calls
// charge only work performed and leave unused EVM gas available. Exhausting
// this envelope is an EVM out-of-gas failure: the enclosing call frame burns
// its remaining gas, including gas above the native cap. This is deliberate
// EVM failure semantics, not a promise that failed calls cost at most 10M.
const nativeExecutionGasCap uint64 = 10_000_000

// ExecuteFrame uses real Evmos cache/snapshot journalling. The environment must
// come from consensus, not the request; this package provides no public RPC.
func ExecuteFrame(db *statedb.StateDB, key storetypes.StoreKey, frame *vm.Contract, readOnly bool, env Environment, request Request) (response Response, err error) {
	if readOnly || frame.Address() != LabAddress || frame.Value().Sign() != 0 {
		return Response{}, errors.New("unsupported static/delegate/payable frame")
	}
	snapshot := db.Snapshot()
	defer func() {
		if p := recover(); p != nil {
			db.RevertToSnapshot(snapshot)
			response = Response{}
			err = errors.New("native frame panic")
			_ = frame.UseGas(frame.Gas)
		}
	}()
	ctx, err := db.GetCacheContext()
	if err != nil {
		return Response{}, err
	}
	oldStore := db.MultiStoreSnapshot()
	oldEvents := ctx.EventManager().Events()
	request.Caller = strings.ToLower(frame.Caller().Hex())
	request.GasLimit = frame.Gas
	if request.GasLimit > nativeExecutionGasCap {
		request.GasLimit = nativeExecutionGasCap
	}
	request.Nonce = env.Nonce
	request.ChainID = env.ChainID
	request.BlockHeight = env.Height
	request.BlockTimestamp = env.Timestamp
	meter := frameMeter{frame: frame, remaining: request.GasLimit}
	var nativeCharged uint64
	// Capture original records for delta accounting. No native write is applied
	// until every charge and response validation below succeeds.
	originals := make(map[string][]byte)
	read := func(k string) ([]byte, error) {
		if !validStateKey(k) {
			return nil, errors.New("invalid state key")
		}
		// Pay lookup/key cost before database access. Value length is available
		// only after Get, but its charge precedes return/allocation across FFI.
		if err := meter.charge(gasCost(candidateReadFlat, candidateReadByte, uint64(len(k)))); err != nil {
			return nil, err
		}
		value := ctx.KVStore(key).Get([]byte(k))
		if err := meter.charge(gasCost(0, candidateReadByte, uint64(len(value)))); err != nil {
			return nil, err
		}
		originals[k] = value
		return value, nil
	}
	response, err = ExecuteMetered(request, read, func(gas uint64) error {
		if err := meter.charge(gas); err != nil {
			return err
		}
		nativeCharged += gas // meter.charge bounds this by the 10M envelope.
		return nil
	})
	if meter.exhausted {
		return Response{}, vm.ErrOutOfGas
	}
	if err != nil {
		_ = frame.UseGas(frame.Gas)
		return Response{}, err
	}
	gas, _ := strconv.ParseUint(response.GasUsed, 10, 64)
	if response.Success && gas != nativeCharged {
		return Response{}, errors.New("native live fuel/report mismatch")
	}
	if !response.Success {
		return response, vm.ErrExecutionReverted
	}
	writes, err := response.DecodedWrites()
	if err != nil {
		return Response{}, err
	}
	keys := make([]string, 0, len(writes))
	for k := range writes {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	for _, k := range keys {
		v := writes[k]
		old, known := originals[k]
		if !known {
			old, err = read(k)
			if err != nil {
				return Response{}, err
			}
		}
		if bytes.Equal(old, v) {
			delete(writes, k) // The host returns whole records even for getters.
			continue
		}
		var oldSize uint64
		if old != nil {
			oldSize = uint64(len(k) + len(old))
		}
		newSize := uint64(len(k) + len(v))
		cost := gasSum(gasCost(candidateWriteFlat, candidateWriteByte, newSize), growthGas(oldSize, newSize))
		if err := meter.charge(cost); err != nil {
			return Response{}, err
		}
	}
	logs, err := json.Marshal(struct{ Deployments, Events []json.RawMessage }{response.Deployments, response.Events})
	if err != nil {
		return Response{}, err
	}
	if len(response.Events) > 0 || len(response.Deployments) > 0 {
		if err := meter.charge(gasCost(candidateLogFlat, candidateLogByte, uint64(len(logs)))); err != nil {
			return Response{}, err
		}
	}
	if err := db.AddPrecompileFn(LabAddress, oldStore, oldEvents); err != nil {
		db.RevertToSnapshot(snapshot)
		return Response{}, err
	}
	for _, k := range keys {
		if v, changed := writes[k]; changed {
			if v == nil {
				ctx.KVStore(key).Delete([]byte(k))
			} else {
				ctx.KVStore(key).Set([]byte(k), v)
			}
		}
	}
	if len(response.Events) > 0 || len(response.Deployments) > 0 {
		db.AddLog(&ethtypes.Log{Address: LabAddress, Topics: []common.Hash{crypto.Keccak256Hash([]byte("LithoNativeLab(bytes)"))}, Data: logs})
	}
	return response, nil
}
