//go:build lithovm_chain_lab

package nativechain

import (
	"math"
	"math/bits"

	"github.com/evmos/evmos/v20/x/evm/core/vm"
)

// Disabled candidate only. Foundation approval and independent review are
// required. Constants are explicit so dependency upgrades cannot change fees.
// KV rates match the pinned SDK; log rates match the pinned EVM LOG1 operation.
const (
	candidateReadFlat  uint64 = 1000
	candidateReadByte  uint64 = 3
	candidateWriteFlat uint64 = 2000
	candidateWriteByte uint64 = 30
	candidateLogFlat   uint64 = 375 + 375
	candidateLogByte   uint64 = 8
	// Proposed persistent allocation floor, NOT an EVM-equivalence claim.
	// Each additional rounded 32-byte key/value unit costs one SSTORE-set rate.
	candidateGrowthUnit uint64 = 20000
)

func gasSum(a, b uint64) uint64 {
	n, carry := bits.Add64(a, b, 0)
	if carry != 0 {
		return math.MaxUint64
	}
	return n
}
func gasCost(flat, rate, size uint64) uint64 {
	high, low := bits.Mul64(rate, size)
	if high != 0 {
		return math.MaxUint64
	}
	return gasSum(flat, low)
}
func allocationUnits(size uint64) uint64 {
	units := size / 32
	if size%32 != 0 {
		units++
	}
	return units
}
func growthGas(oldSize, newSize uint64) uint64 {
	oldUnits, newUnits := allocationUnits(oldSize), allocationUnits(newSize)
	if newUnits <= oldUnits {
		return 0
	} // No refund for shrinking/deleting.
	return gasCost(0, candidateGrowthUnit, newUnits-oldUnits)
}

type frameMeter struct {
	frame     *vm.Contract
	remaining uint64
	exhausted bool
}

func (m *frameMeter) charge(gas uint64) error {
	if m.exhausted || gas > m.remaining || !m.frame.UseGas(gas) {
		m.exhausted = true
		_ = m.frame.UseGas(m.frame.Gas)
		return vm.ErrOutOfGas
	}
	m.remaining -= gas
	return nil
}
