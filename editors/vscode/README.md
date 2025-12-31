# AHA! Language Extension for VS Code

Syntax highlighting for the AHA! programming language.

## Features

- 🎨 Syntax highlighting for `.aha` files
- 📝 Comment support (`//`)
- 🔧 Auto-closing brackets and quotes
- ✨ Built-in function highlighting

## Installation

### From Source
1. Copy `editors/vscode` folder to `~/.vscode/extensions/aha-lang`
2. Restart VS Code
3. Open any `.aha` file

### Via VSIX (Coming Soon)
```bash
code --install-extension aha-lang-0.1.0.vsix
```

## Highlighted Elements

| Element | Color |
|---------|-------|
| Keywords (`let`, `fn`, `if`, `while`, etc.) | Purple |
| Strings | Green |
| Numbers | Orange |
| Functions | Blue |
| Comments | Gray |
| Operators | Red |

## Example

```aha
// Hello World in AHA!
fn main() {
    let message = "Hello, World!";
    print_str(message);
    
    let x = 42;
    print(x);
}
```
