// Patches sing-box source tree for Android compatibility (sing-box v1.13.x).
// Usage: go run ./cmd/patch <path-to-sing-box-source>
package main

import (
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
)

type patch struct {
	file string
	old  string
	new  string
}

var patches = []patch{
	{
		// filemanager.Chown fails on Android with EPERM — make it non-fatal.
		// cachefile still opens and works; only ownership metadata is skipped.
		file: "experimental/cachefile/cache.go",
		old: `	err = filemanager.Chown(c.ctx, c.path)
	if err != nil {
		db.Close()
		return E.Cause(err, "platform chown")
	}`,
		new: `	_ = filemanager.Chown(c.ctx, c.path) // Android: EPERM is non-fatal`,
	},
	{
		old: `	case *http2.Transport:
		connPool := transportConnPool(transport)
		p := (*clientConnPool)((*efaceWords)(unsafe.Pointer(&connPool)).data)
		p.mu.Lock()
		defer p.mu.Unlock()
		for _, vv := range p.conns {
			for _, cc := range vv {
				cc.Close()
			}
		}
		return transport`,
		new: `	case *http2.Transport:
		transport.CloseIdleConnections()
		return transport`,
	},
	{
		old: `//go:linkname transportConnPool golang.org/x/net/http2.(*Transport).connPool
func transportConnPool(t *http2.Transport) http2.ClientConnPool`,
		new: ``,
	},
}

// newFiles re-add the embedded service API that sing-box removed in v1.13.0
// (the mobile lifecycle moved to a daemon+gRPC command server). The Android
// shim only needs a minimal "build config + run box" entry point, so we
// rebuild it over the package-internal box.New / baseContext / platformInterfaceWrapper.
var newFiles = map[string]string{
	"experimental/libbox/zz_whisp_service.go": whispService,
}

const whispService = `package libbox

import (
	"context"

	box "github.com/sagernet/sing-box"
	"github.com/sagernet/sing-box/adapter"
	E "github.com/sagernet/sing/common/exceptions"
	"github.com/sagernet/sing/service"
)

type BoxService struct {
	cancel   context.CancelFunc
	instance *box.Box
}

func NewService(configContent string, platformInterface PlatformInterface) (*BoxService, error) {
	ctx := baseContext(platformInterface)
	options, err := parseConfig(ctx, configContent)
	if err != nil {
		return nil, err
	}
	ctx, cancel := context.WithCancel(ctx)
	platformWrapper := &platformInterfaceWrapper{
		iif:       platformInterface,
		useProcFS: platformInterface.UseProcFS(),
	}
	service.MustRegister[adapter.PlatformInterface](ctx, platformWrapper)
	instance, err := box.New(box.Options{
		Context: ctx,
		Options: options,
	})
	if err != nil {
		cancel()
		return nil, E.Cause(err, "create service")
	}
	return &BoxService{cancel: cancel, instance: instance}, nil
}

func (s *BoxService) Start() error {
	return s.instance.Start()
}

func (s *BoxService) Close() error {
	s.cancel()
	return s.instance.Close()
}
`

func main() {
	if len(os.Args) < 2 {
		fmt.Fprintln(os.Stderr, "usage: patch <sing-box-dir>")
		os.Exit(1)
	}
	root := os.Args[1]
	for _, p := range patches {
		path := filepath.Join(root, filepath.FromSlash(p.file))
		label := p.file
		if p.file == "" {
			found, err := locate(root, p.old)
			if err != nil {
				fmt.Fprintf(os.Stderr, "locate patch target: %v\n", err)
				os.Exit(1)
			}
			path, label = found, strings.TrimSpace(strings.SplitN(p.old, "\n", 2)[0])
		}
		b, err := os.ReadFile(path)
		if err != nil {
			fmt.Fprintf(os.Stderr, "read %s: %v\n", label, err)
			os.Exit(1)
		}
		s := string(b)
		if !strings.Contains(s, p.old) {
			fmt.Fprintf(os.Stderr, "patch target not found for %s — version mismatch?\n", label)
			os.Exit(1)
		}
		s = strings.Replace(s, p.old, p.new, 1)
		if err := os.WriteFile(path, []byte(s), 0644); err != nil {
			fmt.Fprintf(os.Stderr, "write %s: %v\n", label, err)
			os.Exit(1)
		}
		fmt.Printf("patched: %s\n", label)
	}
	for rel, content := range newFiles {
		path := filepath.Join(root, filepath.FromSlash(rel))
		if err := os.WriteFile(path, []byte(content), 0644); err != nil {
			fmt.Fprintf(os.Stderr, "write %s: %v\n", path, err)
			os.Exit(1)
		}
		fmt.Printf("added: %s\n", rel)
	}
}

func locate(root, old string) (string, error) {
	var found []string
	err := filepath.WalkDir(root, func(path string, d fs.DirEntry, err error) error {
		if err != nil || d.IsDir() || !strings.HasSuffix(path, ".go") {
			return err
		}
		b, err := os.ReadFile(path)
		if err == nil && strings.Contains(string(b), old) {
			found = append(found, path)
		}
		return err
	})
	if err != nil {
		return "", err
	}
	if len(found) != 1 {
		return "", fmt.Errorf("%d files hold the patch target", len(found))
	}
	return found[0], nil
}
