package nativechain

import (
	"bytes"
	"encoding/hex"
	"strconv"
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/crypto"
)

func TestGatewayABICanonicalDeployCallAndResult(t *testing.T) {
	code := []byte{1, 2, 3}
	selector := crypto.Keccak256Hash([]byte("initialize(u64)"))
	args := []byte{0x4c, 0x56, 0x41, 0x4c, 1, 0, 0}
	deploy, err := EncodeGatewayDeploy(code, selector, args)
	if err != nil {
		t.Fatal(err)
	}
	decoded, err := DecodeGatewayInput(deploy)
	if err != nil || decoded.Operation != "deploy" || *decoded.Selector != selector.Hex() || *decoded.Bytecode != "0x010203" || decoded.Arguments != "0x4c56414c010000" {
		t.Fatalf("deploy roundtrip: %v %+v", err, decoded)
	}
	contract := common.HexToAddress("0x0000000000000000000000000000000000000009")
	call, err := EncodeGatewayCall(contract, selector, args)
	if err != nil {
		t.Fatal(err)
	}
	decoded, err = DecodeGatewayInput(call)
	if err != nil || decoded.Operation != "call" || *decoded.Contract != contract.Hex() {
		t.Fatalf("call roundtrip: %v %+v", err, decoded)
	}
	output, err := EncodeGatewayResult("0x010203")
	if err != nil {
		t.Fatal(err)
	}
	result, err := DecodeGatewayResult(output)
	if err != nil || !bytes.Equal(result, code) {
		t.Fatalf("result roundtrip: %v %x", err, result)
	}
}

func TestGatewayABIRejectsMalleableAndOversizedPayloads(t *testing.T) {
	selector := crypto.Keccak256Hash([]byte("get()"))
	valid, err := EncodeGatewayCall(common.HexToAddress("0x09"), selector, []byte{1})
	if err != nil {
		t.Fatal(err)
	}
	for _, input := range [][]byte{
		nil,
		[]byte{1, 2, 3, 4},
		append(bytes.Clone(valid), 0),
		append(bytes.Clone(valid), bytes.Repeat([]byte{0}, 32)...),
	} {
		if _, err := DecodeGatewayInput(input); err == nil {
			t.Fatalf("accepted malformed payload: %s", hex.EncodeToString(input))
		}
	}
	badOffset := bytes.Clone(valid)
	badOffset[4+64+31]++
	if _, err := DecodeGatewayInput(badOffset); err == nil {
		t.Fatal("accepted offset alias")
	}
	if _, err := EncodeGatewayCall(common.Address{}, [32]byte{}, nil); err == nil {
		t.Fatal("accepted zero call selector")
	}
	if _, err := EncodeGatewayDeploy([]byte{1}, [32]byte{}, []byte{1}); err == nil {
		t.Fatal("accepted arguments without initializer")
	}
	if _, err := EncodeGatewayDeploy(bytes.Repeat([]byte{1}, 65537), selector, nil); err == nil {
		t.Fatal("accepted oversized code")
	}
	if _, err := EncodeGatewayDeploy([]byte{1}, selector, bytes.Repeat([]byte{1}, MaxBytes)); err == nil {
		t.Fatal("accepted oversized ABI")
	}
	result, err := EncodeGatewayResult("0x01")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := DecodeGatewayResult(append(result, 0)); err == nil {
		t.Fatal("accepted trailing result bytes")
	}
}

func FuzzGatewayABICanonical(f *testing.F) {
	selector := crypto.Keccak256Hash([]byte("initialize(u64)"))
	deploy, err := EncodeGatewayDeploy([]byte{1, 2, 3}, selector, []byte{4, 5})
	if err != nil {
		f.Fatal(err)
	}
	call, err := EncodeGatewayCall(common.HexToAddress("0x09"), selector, []byte{6, 7})
	if err != nil {
		f.Fatal(err)
	}
	f.Add(deploy)
	f.Add(call)
	f.Add([]byte{1, 2, 3, 4})
	f.Fuzz(func(t *testing.T, input []byte) {
		request, err := DecodeGatewayInput(input)
		if err != nil {
			return
		}
		selectorBytes, err := hex.DecodeString((*request.Selector)[2:])
		if err != nil {
			t.Fatal(err)
		}
		var selected [32]byte
		copy(selected[:], selectorBytes)
		arguments, err := hex.DecodeString(request.Arguments[2:])
		if err != nil {
			t.Fatal(err)
		}
		var reencoded []byte
		if request.Operation == "deploy" {
			code, err := hex.DecodeString((*request.Bytecode)[2:])
			if err != nil {
				t.Fatal(err)
			}
			reencoded, err = EncodeGatewayDeploy(code, selected, arguments)
			if err != nil {
				t.Fatal(err)
			}
		} else {
			reencoded, err = EncodeGatewayCall(common.HexToAddress(*request.Contract), selected, arguments)
			if err != nil {
				t.Fatal(err)
			}
		}
		if !bytes.Equal(input, reencoded) {
			t.Fatal("accepted noncanonical ABI")
		}
	})
}

func BenchmarkGatewayDecode(b *testing.B) {
	selector := crypto.Keccak256Hash([]byte("initialize(u64)"))
	for _, size := range []int{128, 65536} {
		payload, err := EncodeGatewayDeploy(bytes.Repeat([]byte{1}, size), selector, []byte{1, 2, 3})
		if err != nil {
			b.Fatal(err)
		}
		b.Run(strconv.Itoa(size), func(b *testing.B) {
			b.SetBytes(int64(len(payload)))
			for i := 0; i < b.N; i++ {
				if _, err := DecodeGatewayInput(payload); err != nil {
					b.Fatal(err)
				}
			}
		})
	}
}
