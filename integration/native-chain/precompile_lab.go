//go:build lithovm_chain_lab

package nativechain

import (
	"errors"

	storetypes "cosmossdk.io/store/types"
	"github.com/ethereum/go-ethereum/common"
	"github.com/evmos/evmos/v20/x/evm/core/vm"
	"github.com/evmos/evmos/v20/x/evm/statedb"
)

// GatewayTransaction must be built by the chain keeper from the EVM message
// after ante validation, or for discard-only simulation. Never from calldata.
type GatewayTransaction struct {
	Origin    common.Address
	Nonce     uint64
	ChainID   uint64
	Simulated bool
}

// LabPrecompile is deliberately registered only by tests. Its gas pricing and
// receipt log are not approved consensus rules.
type LabPrecompile struct {
	Key         storetypes.StoreKey
	Transaction GatewayTransaction
}

var _ vm.PrecompiledContract = LabPrecompile{}

func (LabPrecompile) Address() common.Address { return LabAddress }

// Decode work is prepaid even for malformed input. ExecuteFrame separately
// charges native execution and candidate KV/growth/log pricing. Not approved.
func (LabPrecompile) RequiredGas(input []byte) uint64 {
	return 500 + 4*uint64(len(input))
}

func (p LabPrecompile) Run(evm *vm.EVM, frame *vm.Contract, readOnly bool) ([]byte, error) {
	db, ok := evm.StateDB.(*statedb.StateDB)
	if !ok || evm.Context.BlockNumber == nil || evm.Context.Time == nil ||
		!evm.Context.BlockNumber.IsUint64() || !evm.Context.Time.IsUint64() {
		return nil, errors.New("unsupported lab EVM context")
	}
	if p.Transaction.Origin == (common.Address{}) || p.Transaction.Origin != evm.TxContext.Origin || p.Transaction.ChainID == 0 {
		return nil, errors.New("unauthenticated lab transaction context")
	}
	request, err := DecodeGatewayInput(frame.Input)
	if err != nil {
		return nil, errors.New("invalid lab payload")
	}
	// The host still derives its caller from the immediate frame. Origin is used
	// only to disallow top-level deployment through an EVM wrapper.
	if request.Operation == "deploy" && (frame.Caller() != p.Transaction.Origin || evm.StateDB.GetCodeSize(frame.Caller()) != 0) {
		return nil, vm.ErrExecutionReverted
	}
	env := Environment{
		ChainID:   p.Transaction.ChainID,
		Nonce:     p.Transaction.Nonce,
		Height:    evm.Context.BlockNumber.Uint64(),
		Timestamp: evm.Context.Time.Uint64(),
	}
	response, err := ExecuteFrame(db, p.Key, frame, readOnly, env, request)
	if err != nil {
		return nil, err
	}
	return EncodeGatewayResult(response.Result)
}

// RegisterLabPrecompile modifies only the supplied ephemeral EVM instance.
// It is never called from the chain app or production precompile registry.
func RegisterLabPrecompile(evm *vm.EVM, key storetypes.StoreKey, tx GatewayTransaction) {
	p := LabPrecompile{Key: key, Transaction: tx}
	evm.WithPrecompiles(map[common.Address]vm.PrecompiledContract{LabAddress: p}, []common.Address{LabAddress})
}
