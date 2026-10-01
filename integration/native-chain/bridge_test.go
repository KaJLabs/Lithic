package nativechain

import (
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os/exec"
	"reflect"
	"strconv"
	"strings"
	"testing"
)

func ptr(s string) *string { return &s }
func fixture(t testing.TB, source string) string {
	t.Helper()
	out, err := exec.Command("../../target/debug/lithc", "--emit", "lithovm", source).CombinedOutput()
	if err != nil {
		t.Fatalf("compile fixture: %s: %v", out, err)
	}
	var artifact struct {
		Bytecode string `json:"bytecode"`
	}
	if err := json.Unmarshal(out, &artifact); err != nil {
		t.Fatal(err)
	}
	return artifact.Bytecode
}
func args(numbers ...uint64) string {
	out := fmt.Sprintf("0x4c56414c01%04x", len(numbers))
	for i, n := range numbers {
		tag := 1
		if i == 1 {
			tag = 3
		}
		out += fmt.Sprintf("%02x%064x", tag, n)
	}
	return out
}
func request(t testing.TB) Request {
	return Request{Version: 1, Operation: "deploy", Caller: "0x0000000000000000000000000000000000000009", Bytecode: ptr(fixture(t, "testdata/counter.lithic")), Function: ptr("initialize"), Arguments: args(7), Nonce: 1, GasLimit: 1000000, ChainID: 700777, BlockHeight: 1, BlockTimestamp: 2}
}
func returnedAddress(t testing.TB, response Response) string {
	t.Helper()
	bytes, err := hex.DecodeString(strings.TrimPrefix(response.Result, "0x"))
	if err != nil || len(bytes) != 40 || bytes[7] != 4 {
		t.Fatalf("not address envelope: %s", response.Result)
	}
	return "0x" + hex.EncodeToString(bytes[20:])
}
func TestGoRustPersistenceFailureAndMalformedRequests(t *testing.T) {
	state := map[string][]byte{}
	reader := func(k string) ([]byte, error) { return state[k], nil }
	req := request(t)
	result, err := Execute(req, reader)
	if err != nil || !result.Success {
		t.Fatalf("deploy: %+v %v", result, err)
	}
	if len(state) != 0 || len(result.Deployments) != 1 {
		t.Fatal("execution wrote externally or omitted registration")
	}
	writes, err := result.DecodedWrites()
	if err != nil {
		t.Fatal(err)
	}
	for k, v := range writes {
		state[k] = v
	}
	address := returnedAddress(t, result)
	req.Operation = "call"
	req.Bytecode = nil
	req.Contract = &address
	req.Function = ptr("set")
	req.Arguments = args(42, 0)
	failed, err := Execute(req, reader)
	if err != nil || failed.Success || len(failed.Writes) != 0 || len(failed.Events) != 0 {
		t.Fatalf("failure leaked: %+v %v", failed, err)
	}
	req.Arguments = args(42, 1)
	success, err := Execute(req, reader)
	if err != nil || !success.Success {
		t.Fatalf("set: %+v %v", success, err)
	}
	changed, _ := success.DecodedWrites()
	for k, v := range changed {
		state[k] = v
	}
	req.Function = ptr("get")
	req.Arguments = args()
	read, err := Execute(req, reader)
	if err != nil || !read.Success || !strings.HasSuffix(read.Result, fmt.Sprintf("%064x", 42)) {
		t.Fatalf("reload: %+v %v", read, err)
	}
	req.Version = 2
	if _, err := Execute(req, reader); err == nil {
		t.Fatal("accepted version")
	}
	req.Version = 1
	for k := range state {
		state[k] = []byte("corrupt")
	}
	corrupt, err := Execute(req, reader)
	if err != nil || corrupt.Success || len(corrupt.Writes) != 0 {
		t.Fatal("accepted corrupt persistence")
	}
	panicking := func(string) ([]byte, error) { panic("test callback") }
	trapped, err := Execute(req, panicking)
	if err != nil || trapped.Success {
		t.Fatal("callback panic escaped or succeeded")
	}
	if !reflect.DeepEqual(failed.Deployments, []json.RawMessage{}) {
		t.Fatal("failed registration")
	}
}

func TestLiveFuelCallbackMatchesNativeReportAndRejectsExhaustion(t *testing.T) {
	req := request(t)
	reader := func(string) ([]byte, error) { return nil, nil }
	var charged uint64
	result, err := ExecuteMetered(req, reader, func(amount uint64) error {
		charged += amount
		return nil
	})
	if err != nil || !result.Success {
		t.Fatalf("metered deployment failed: %v %+v", err, result)
	}
	reported, err := strconv.ParseUint(result.GasUsed, 10, 64)
	if err != nil || charged != reported || charged == 0 {
		t.Fatalf("live charge %d differs from native report %d: %v", charged, reported, err)
	}
	remaining := charged - 1
	failed, err := ExecuteMetered(req, reader, func(amount uint64) error {
		if amount > remaining {
			return fmt.Errorf("fuel exhausted")
		}
		remaining -= amount
		return nil
	})
	if err != nil || failed.Success || len(failed.Writes) != 0 || len(failed.Deployments) != 0 {
		t.Fatalf("exhaustion leaked effects: %v %+v", err, failed)
	}
}

func TestDeclaredInitializerCannotBeOmittedOrBypassed(t *testing.T) {
	for _, function := range []*string{nil, ptr("get")} {
		req := request(t)
		req.Function = function
		req.Arguments = args()
		result, err := Execute(req, func(string) ([]byte, error) { return nil, nil })
		if err != nil || result.Success || len(result.Writes) != 0 || len(result.Deployments) != 0 || len(result.Events) != 0 {
			t.Fatalf("uninitialized deployment escaped: error=%v result=%+v", err, result)
		}
	}
}
