//go:build lithovm_chain_lab && !lithovm_bank_candidate

package nativechain

import (
	storetypes "cosmossdk.io/store/types"
	"github.com/evmos/evmos/v20/x/evm/core/vm"
	"github.com/evmos/evmos/v20/x/evm/statedb"
)

const NativeCustodyCandidateEnabled = false

func executeSelectedProfile(db *statedb.StateDB, key storetypes.StoreKey, frame *vm.Contract, readOnly bool, env Environment, request Request) (Response, error) {
	return ExecuteFrame(db, key, frame, readOnly, env, request)
}
