//go:build lithovm_chain_lab

package nativechain

import (
	"errors"
	"math/big"

	storetypes "cosmossdk.io/store/types"
	"github.com/ethereum/go-ethereum/common"
	"github.com/evmos/evmos/v20/x/evm/core/core"
	"github.com/evmos/evmos/v20/x/evm/core/vm"
)

// RegisterLabForMessage is the proposed keeper seam, exercised only by tests.
// The caller must supply the message after ante verification for a committed
// transaction. A fake eth_call/estimate message may only use discard mode.
func RegisterLabForMessage(evm *vm.EVM, key storetypes.StoreKey, msg core.Message, chainID *big.Int, commit bool) error {
	if evm == nil || msg == nil || chainID == nil || !chainID.IsUint64() || chainID.Sign() <= 0 {
		return errors.New("invalid native gateway transaction context")
	}
	if msg.From() != evm.TxContext.Origin || msg.From() == (common.Address{}) {
		return errors.New("EVM origin does not match message sender")
	}
	if msg.IsFake() && commit {
		return errors.New("unauthenticated simulation cannot commit")
	}
	RegisterLabPrecompile(evm, key, GatewayTransaction{
		Origin: msg.From(), Nonce: msg.Nonce(), ChainID: chainID.Uint64(), Simulated: msg.IsFake(),
	})
	return nil
}
