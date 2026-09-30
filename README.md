Kex Programming Language (.kx)

Kex (.kx) is a lightweight, interpreted programming language written in Rust. Designed with developer experience in mind, Kex features clean error diagnostics, ANSI-colored output, a Windows setup installer, and official VS Code integration with syntax highlighting out of the box.

📸 Overview & Features

Global CLI (kex): Run Kex scripts from any terminal or directory seamlessly.

Rust Core: Fast tokenization, parsing, and execution engine.

Rich Developer Tooling: Includes an Inno Setup installer that manages system PATH updates and auto-installs the custom VS Code extension (.vsix).

Pretty Diagnostics: Colored syntax error and runtime diagnostics specifying line numbers and token locations.

🚀 Quick Start & Installation

Option 1: Windows Installer (Recommended)

Download KexSetup.exe from the Releases page or compile installer.iss using Inno Setup.

Run KexSetup.exe.

Ensure "Add Kex to System PATH" is checked during setup.

Restart your terminal or VS Code to refresh environment variables.

Option 2: Build from Source

Prerequisites

Rust & Cargo (1.70+)

Inno Setup 6+ (Optional, for building the installer)

Build Steps

# Clone the repository
git clone https://github.com/click-for-games/Kex.git
cd Kex

# Build release binary
cargo build --release

# The binary will be available at target/release/kex.exe


📖 Kex Language Documentation

1. Comments

Kex supports both single-line and multi-line comments:

// This is a single-line comment

/*
   This is a multi-line comment
   spanning multiple lines.
*/


2. Variables & Constants

Variables are declared using the let keyword:

let message = "Hello, Kex!";
let count = 42;
let pi = 3.14;


3. Console I/O

Output (kxout): Prints a value or expression to standard output.

Input (kxin): Reads a line of user input from the console.

kxout("Enter your name:");
let name = kxin;
kxout("Hello, ");
kxout(name);


4. Type Casting (cast)

Use the cast function to convert values between data types ("int", "float", "string"):

kxout("Enter a number:");
let input = kxin;
let num = cast(input, "float");

// Perform numeric operations on casted values
let doubleValue = num * 2.0;
kxout(doubleValue);


5. Control Flow

Kex supports if and else conditional blocks:

let score = 85;

if (score >= 50) {
    kxout("Passed!");
} else {
    kxout("Failed!");
}


Note: For nested conditions, place new if statements inside explicit else blocks:

if (op == "+") {
    kxout(a + b);
} else {
    if (op == "-") {
        kxout(a - b);
    }
}


6. Functions

Define custom functions using the fn keyword:

fn greet(user) {
    kxout("Welcome back, ");
    kxout(user);
}

greet("Alex");


🛠️ VS Code Integration

The project includes a custom VS Code extension with syntax highlighting for .kx files (vscode-extension/).

Manual Extension Installation

If you aren't using the Windows installer, install the extension manually using the VS Code CLI:

code --install-extension vscode-extension/kex-lang-1.0.0.vsix --force


🧪 Running Examples

Create a file named hello.kx:

// hello.kx
kxout("Hello, World from Kex!");


Execute it via terminal:

kex hello.kx


🤝 Contributing

Contributions are very welcome! Whether it's fixing open issues, improving language documentation, or expanding language features, feel free to submit a Pull Request.

Development Workflow

Fork the project repository.

Create your feature branch (git checkout -b feature/AmazingFeature).

Commit your changes (git commit -m 'Add some AmazingFeature').

Push to the branch (git push origin feature/AmazingFeature).

Open a Pull Request.

📄 License

Distributed under the MIT License. See LICENSE for more information.
