//go:build lithovm_bank_candidate

package nativechain

/*
#cgo CFLAGS: -I${SRCDIR}/../../packages/vm/lithovm-ffi/include
#include "lithovm.h"
extern intptr_t lithovmGoRead(uintptr_t, uint8_t *, size_t, uint8_t *, size_t);
extern int32_t lithovmGoCharge(uintptr_t, uint64_t);
extern int32_t lithovmGoBalance(uintptr_t, uint8_t *, uint8_t *);
static int32_t executeGoCustody(const uint8_t *input, size_t len, uintptr_t handle, LithoBuffer *output) {
    return lithovm_execute_v3(input, len, handle, (LithoRead)lithovmGoRead, (LithoReadBalance)lithovmGoBalance, (LithoCharge)lithovmGoCharge, output);
}
*/
import "C"

import (
	"bytes"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math/big"
	"runtime/cgo"
	"strings"
	"unsafe"
)

type BalanceChange struct {
	Address string `json:"address"`
	Before  string `json:"before"`
	After   string `json:"after"`
}
type BankResponse struct {
	Response
	BankVersion int             `json:"bankVersion"`
	Balances    []BalanceChange `json:"balances"`
}

//export lithovmGoBalance
func lithovmGoBalance(handle C.uintptr_t, address *C.uint8_t, output *C.uint8_t) (result C.int32_t) {
	defer func() {
		if recover() != nil {
			result = 0
		}
	}()
	if address == nil || output == nil {
		return 0
	}
	state := cgo.Handle(handle).Value().(callbacks)
	if state.balance == nil {
		return 0
	}
	var key [20]byte
	copy(key[:], unsafe.Slice((*byte)(unsafe.Pointer(address)), 20))
	value, err := state.balance(key)
	if err != nil {
		return 0
	}
	copy(unsafe.Slice((*byte)(unsafe.Pointer(output)), 32), value[:])
	return 1
}

// ExecuteCustody returns a validated, staged bank batch. Neither this interface
// nor the C ABI applies external state. The embedding frame owns the journal.
func ExecuteCustody(request Request, read Reader, balance BalanceReader, charge Charger) (BankResponse, error) {
	var result BankResponse
	if request.Version != 2 || read == nil || balance == nil || charge == nil {
		return result, errors.New("custody request/readers/fuel required")
	}
	raw, err := json.Marshal(request)
	if err != nil || len(raw) > MaxBytes {
		return result, errors.New("invalid custody request")
	}
	handle := cgo.NewHandle(callbacks{read: read, charge: charge, balance: balance})
	defer handle.Delete()
	var output C.LithoBuffer
	code := C.executeGoCustody((*C.uint8_t)(unsafe.Pointer(&raw[0])), C.size_t(len(raw)), C.uintptr_t(handle), &output)
	if code != 0 {
		return result, fmt.Errorf("custody FFI rejected request: %d", code)
	}
	defer C.lithovm_free_v1(output)
	if output.data == nil || output.len > MaxBytes {
		return result, errors.New("invalid custody buffer")
	}
	decoder := json.NewDecoder(bytes.NewReader(C.GoBytes(unsafe.Pointer(output.data), C.int(output.len))))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&result); err != nil {
		return BankResponse{}, err
	}
	if decoder.Decode(new(any)) != io.EOF {
		return BankResponse{}, errors.New("trailing custody response")
	}
	if _, err := validateResponse(result.Response, request); err != nil {
		return BankResponse{}, err
	}
	if _, err := result.DecodedBalances(); err != nil {
		return BankResponse{}, err
	}
	return result, nil
}

type decodedBalance struct {
	address       [20]byte
	before, after *big.Int
}

func (response BankResponse) DecodedBalances() ([]decodedBalance, error) {
	if response.BankVersion != 1 || response.Balances == nil || len(response.Balances) > 128 || (!response.Success && len(response.Balances) != 0) {
		return nil, errors.New("invalid custody schema/effect count")
	}
	var previous string
	beforeSum, afterSum := new(big.Int), new(big.Int)
	result := make([]decodedBalance, 0, len(response.Balances))
	for _, entry := range response.Balances {
		canonical := func(value string, length int) bool {
			return strings.HasPrefix(value, "0x") && canonicalHex(value[2:], length)
		}
		if !canonical(entry.Address, 40) || !canonical(entry.Before, 64) || !canonical(entry.After, 64) || entry.Before == entry.After || (previous != "" && entry.Address <= previous) {
			return nil, errors.New("noncanonical or duplicate custody update")
		}
		raw, _ := hex.DecodeString(entry.Address[2:])
		var address [20]byte
		copy(address[:], raw)
		before, _ := new(big.Int).SetString(entry.Before[2:], 16)
		after, _ := new(big.Int).SetString(entry.After[2:], 16)
		beforeSum.Add(beforeSum, before)
		afterSum.Add(afterSum, after)
		result = append(result, decodedBalance{address, before, after})
		previous = entry.Address
	}
	if beforeSum.Cmp(afterSum) != 0 {
		return nil, errors.New("custody batch changes total bank balance")
	}
	return result, nil
}
