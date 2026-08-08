package doctest

import (
	"context"
	"fmt"
	"go/ast"
	"go/parser"
	"go/token"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"sort"
	"strings"
	"testing"
	"time"
)

const markedFence = "``` { .go .blkit-example title=\"main.go\" }"

type fragment struct {
	line int
	body string
}

func repositoryRoot(t *testing.T) string {
	t.Helper()
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("locate doctest source")
	}
	return filepath.Clean(filepath.Join(filepath.Dir(file), "..", ".."))
}

func extract(path string, data []byte) ([]fragment, error) {
	lines := strings.SplitAfter(string(data), "\n")
	var fragments []fragment
	for i := 0; i < len(lines); i++ {
		line := strings.TrimSuffix(strings.TrimSuffix(lines[i], "\n"), "\r")
		if !strings.Contains(line, ".blkit-example") {
			continue
		}
		if line != markedFence {
			return nil, fmt.Errorf("%s:%d: malformed executable Go fence", path, i+1)
		}
		start := i + 2
		var body strings.Builder
		i++
		for ; i < len(lines); i++ {
			line = strings.TrimSuffix(strings.TrimSuffix(lines[i], "\n"), "\r")
			if line == "```" {
				break
			}
			body.WriteString(lines[i])
		}
		if i == len(lines) {
			return nil, fmt.Errorf("%s:%d: unclosed executable Go fence", path, start-1)
		}
		fragments = append(fragments, fragment{line: start, body: body.String()})
	}
	return fragments, nil
}

func assemble(path string, fragments []fragment) string {
	var source strings.Builder
	for _, f := range fragments {
		fmt.Fprintf(&source, "//line %s:%d\n", filepath.ToSlash(path), f.line)
		source.WriteString(f.body)
		if !strings.HasSuffix(f.body, "\n") {
			source.WriteByte('\n')
		}
		source.WriteByte('\n')
	}
	return source.String()
}

func fixtureTests(path string) (logic, command bool, err error) {
	file, err := parser.ParseFile(token.NewFileSet(), path, nil, 0)
	if err != nil {
		return false, false, err
	}
	if file.Name.Name != "main" {
		return false, false, fmt.Errorf("fixture package is %q; want main", file.Name.Name)
	}
	for _, decl := range file.Decls {
		fn, ok := decl.(*ast.FuncDecl)
		if !ok || fn.Recv != nil {
			continue
		}
		logic = logic || strings.HasPrefix(fn.Name.Name, "TestLogic")
		command = command || strings.HasPrefix(fn.Name.Name, "TestCommand")
	}
	return logic, command, nil
}

func runGo(t *testing.T, root string, env []string, args ...string) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Minute)
	defer cancel()
	cmd := exec.CommandContext(ctx, "go", args...)
	cmd.Dir = root
	cmd.Env = append(os.Environ(), env...)
	output, err := cmd.CombinedOutput()
	if ctx.Err() != nil {
		t.Fatalf("go %s: %v", strings.Join(args, " "), ctx.Err())
	}
	if err != nil {
		t.Fatalf("go %s: %v\n%s", strings.Join(args, " "), err, output)
	}
}

func TestExamples(t *testing.T) {
	root := repositoryRoot(t)
	pages, err := filepath.Glob(filepath.Join(root, "docs", "examples", "*.md"))
	if err != nil {
		t.Fatal(err)
	}
	sort.Strings(pages)

	fixtureRoot := filepath.Join(root, "internal", "doctest", "testdata")
	fixtureEntries, err := os.ReadDir(fixtureRoot)
	if err != nil && !os.IsNotExist(err) {
		t.Fatal(err)
	}
	fixtures := map[string]string{}
	for _, entry := range fixtureEntries {
		if !entry.IsDir() {
			continue
		}
		path := filepath.Join(fixtureRoot, entry.Name(), "example_test.go")
		if _, err := os.Stat(path); err == nil {
			fixtures[entry.Name()] = path
		}
	}

	seen := map[string]bool{}
	for _, page := range pages {
		name := strings.TrimSuffix(filepath.Base(page), ".md")
		data, err := os.ReadFile(page)
		if err != nil {
			t.Fatal(err)
		}
		fragments, err := extract(page, data)
		if err != nil {
			t.Error(err)
			continue
		}
		fixture, hasFixture := fixtures[name]
		if len(fragments) == 0 {
			if hasFixture {
				t.Errorf("%s: test fixture exists without executable Go source", page)
			}
			continue
		}
		seen[name] = true
		if !hasFixture {
			t.Errorf("%s: executable Go source has no test fixture", page)
			continue
		}

		t.Run(name, func(t *testing.T) {
			logic, command, err := fixtureTests(fixture)
			if err != nil {
				t.Fatalf("parse fixture: %v", err)
			}
			if !logic || !command {
				t.Fatalf("fixture must define TestLogic... and TestCommand... tests")
			}

			temp := t.TempDir()
			mainPath := filepath.Join(temp, "main.go")
			testPath := filepath.Join(temp, "example_test.go")
			if err := os.WriteFile(mainPath, []byte(assemble(page, fragments)), 0o600); err != nil {
				t.Fatal(err)
			}
			testData, err := os.ReadFile(fixture)
			if err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(testPath, testData, 0o600); err != nil {
				t.Fatal(err)
			}

			binary := filepath.Join(temp, "example")
			if runtime.GOOS == "windows" {
				binary += ".exe"
			}
			runGo(t, root, nil, "build", "-o", binary, mainPath)
			runGo(t, root, []string{"BLKIT_EXAMPLE_BINARY=" + binary}, "test", mainPath, testPath)
		})
	}
	for name, path := range fixtures {
		if !seen[name] {
			t.Errorf("%s: fixture has no matching executable page", path)
		}
	}
}

func TestExtract(t *testing.T) {
	tests := []struct {
		name    string
		input   string
		count   int
		wantErr bool
	}{
		{name: "none", input: "```go\nignored\n```\n"},
		{name: "marked", input: markedFence + "\npackage main\n```\n", count: 1},
		{name: "two", input: markedFence + "\na\n```\n" + markedFence + "\nb\n```\n", count: 2},
		{name: "wrong title", input: "``` { .go .blkit-example title=\"other.go\" }\n```\n", wantErr: true},
		{name: "unclosed", input: markedFence + "\npackage main\n", wantErr: true},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			got, err := extract("example.md", []byte(test.input))
			if (err != nil) != test.wantErr {
				t.Fatalf("extract error = %v, wantErr %v", err, test.wantErr)
			}
			if len(got) != test.count {
				t.Fatalf("fragment count = %d, want %d", len(got), test.count)
			}
		})
	}
}
