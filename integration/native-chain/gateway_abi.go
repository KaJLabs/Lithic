package nativechain

import (
	"bytes"
	"encoding/hex"
	"errors"

	"github.com/ethereum/go-ethereum/accounts/abi"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/crypto"
)

const (
	deploySignature = "deploy(bytes,bytes32,bytes)"
	callSignature   = "call(address,bytes32,bytes)"
)

func gatewayArguments(kind string) (abi.Arguments, error) {
	first := "bytes"
	if kind == "call" {
		first = "address"
	}
	firstType, err := abi.NewType(first, "", nil)
	if err != nil {
		return nil, err
	}
	selectorType, err := abi.NewType("bytes32", "", nil)
	if err != nil {
		return nil, err
	}
	bytesType, err := abi.NewType("bytes", "", nil)
	if err != nil {
		return nil, err
	}
	return abi.Arguments{{Type: firstType}, {Type: selectorType}, {Type: bytesType}}, nil
}

func methodID(signature string) []byte { return crypto.Keccak256([]byte(signature))[:4] }

func encodeGateway(kind string, target interface{}, selector [32]byte, arguments []byte) ([]byte, error) {
	if len(arguments) > MaxBytes {
		return nil, errors.New("native arguments exceed gateway limit")
	}
	if kind == "deploy" {
		code, ok := target.([]byte)
		if !ok || len(code) == 0 || len(code) > 65536 {
			return nil, errors.New("invalid native bytecode length")
		}
		if selector == [32]byte{} && len(arguments) != 0 {
			return nil, errors.New("arguments require initializer")
		}
	} else if selector == [32]byte{} {
		return nil, errors.New("call requires a native selector")
	}
	args, err := gatewayArguments(kind)
	if err != nil {
		return nil, err
	}
	body, err := args.Pack(target, selector, arguments)
	if err != nil {
		return nil, err
	}
	signature := deploySignature
	if kind == "call" {
		signature = callSignature
	}
	payload := append(methodID(signature), body...)
	if len(payload) > MaxBytes {
		return nil, errors.New("gateway payload exceeds limit")
	}
	return payload, nil
}

// EncodeGatewayDeploy wraps bytecode, a full native selector (zero for no
// initializer), and native-encoded arguments in a canonical EVM ABI call.
func EncodeGatewayDeploy(code []byte, selector [32]byte, arguments []byte) ([]byte, error) {
	return encodeGateway("deploy", code, selector, arguments)
}

func EncodeGatewayCall(contract common.Address, selector [32]byte, arguments []byte) ([]byte, error) {
	return encodeGateway("call", contract, selector, arguments)
}

// DecodeGatewayInput rejects noncanonical offsets, trailing bytes, and
// underspecified selectors by requiring a byte-identical ABI re-encoding.
func DecodeGatewayInput(input []byte) (Request, error) {
	if len(input) < 4 || len(input) > MaxBytes {
		return Request{}, errors.New("invalid gateway payload length")
	}
	kind := ""
	switch {
	case bytes.Equal(input[:4], methodID(deploySignature)):
		kind = "deploy"
	case bytes.Equal(input[:4], methodID(callSignature)):
		kind = "call"
	default:
		return Request{}, errors.New("unknown gateway method")
	}
	args, err := gatewayArguments(kind)
	if err != nil {
		return Request{}, err
	}
	values, err := args.Unpack(input[4:])
	if err != nil || len(values) != 3 {
		return Request{}, errors.New("invalid gateway arguments")
	}
	selector, ok := values[1].([32]byte)
	if !ok {
		return Request{}, errors.New("invalid native selector")
	}
	arguments, ok := values[2].([]byte)
	if !ok {
		return Request{}, errors.New("invalid native arguments")
	}
	canonical, err := args.Pack(values...)
	if err != nil || !bytes.Equal(canonical, input[4:]) {
		return Request{}, errors.New("noncanonical gateway ABI")
	}
	selectorHex := "0x" + hex.EncodeToString(selector[:])
	request := Request{Version: 1, Operation: kind, Selector: &selectorHex, Arguments: "0x" + hex.EncodeToString(arguments)}
	if kind == "deploy" {
		code, ok := values[0].([]byte)
		if !ok || len(code) == 0 || len(code) > 65536 {
			return Request{}, errors.New("invalid native bytecode length")
		}
		if selector == [32]byte{} && len(arguments) != 0 {
			return Request{}, errors.New("arguments require initializer")
		}
		codeHex := "0x" + hex.EncodeToString(code)
		request.Bytecode = &codeHex
	} else {
		contract, ok := values[0].(common.Address)
		if !ok || selector == [32]byte{} {
			return Request{}, errors.New("invalid native target or selector")
		}
		address := contract.Hex()
		request.Contract = &address
	}
	return request, nil
}

func EncodeGatewayResult(nativeResult string) ([]byte, error) {
	result, err := hex.DecodeString(stripHexPrefix(nativeResult))
	if err != nil {
		return nil, err
	}
	bytesType, err := abi.NewType("bytes", "", nil)
	if err != nil {
		return nil, err
	}
	return (abi.Arguments{{Type: bytesType}}).Pack(result)
}

func DecodeGatewayResult(output []byte) ([]byte, error) {
	bytesType, err := abi.NewType("bytes", "", nil)
	if err != nil {
		return nil, err
	}
	arguments := abi.Arguments{{Type: bytesType}}
	values, err := arguments.Unpack(output)
	if err != nil || len(values) != 1 {
		return nil, errors.New("invalid gateway result")
	}
	canonical, err := arguments.Pack(values...)
	if err != nil || !bytes.Equal(output, canonical) {
		return nil, errors.New("noncanonical gateway result")
	}
	result, ok := values[0].([]byte)
	if !ok {
		return nil, errors.New("invalid native result")
	}
	return result, nil
}

func stripHexPrefix(value string) string {
	if len(value) >= 2 && value[:2] == "0x" {
		return value[2:]
	}
	return value
}
