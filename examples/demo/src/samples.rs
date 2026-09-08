//! Per-language sample snippets shown in the dropdown. Short, original, and written to
//! exercise the highlighter (keywords, types, strings, numbers, comments, functions).

pub struct Sample {
    pub name: &'static str,
    pub file: &'static str,
    pub src: &'static str,
}

pub const SAMPLES: &[Sample] = &[
    Sample {
        name: "Rust",
        file: "main.rs",
        src: "use std::collections::HashMap;\n\n/// Count how many times each word appears.\nfn word_counts(text: &str) -> HashMap<String, u32> {\n    let mut counts = HashMap::new();\n    for word in text.split_whitespace() {\n        *counts.entry(word.to_lowercase()).or_insert(0) += 1;\n    }\n    counts\n}\n\nfn main() {\n    let counts = word_counts(\"the cat the hat\");\n    println!(\"unique words: {}\", counts.len());\n}\n",
    },
    Sample {
        name: "TypeScript",
        file: "store.ts",
        src: "interface Todo {\n  id: number;\n  title: string;\n  done: boolean;\n}\n\nexport class Store {\n  private todos: Todo[] = [];\n\n  add(title: string): Todo {\n    const todo = { id: this.todos.length + 1, title, done: false };\n    this.todos.push(todo);\n    return todo;\n  }\n\n  get remaining(): number {\n    return this.todos.filter((t) => !t.done).length;\n  }\n}\n",
    },
    Sample {
        name: "JavaScript",
        file: "fetch.js",
        src: "// Fetch a user and greet them.\nasync function greet(id) {\n  const res = await fetch(`/api/users/${id}`);\n  if (!res.ok) throw new Error(\"not found\");\n  const user = await res.json();\n  return `Hello, ${user.name}!`;\n}\n\ngreet(42).then(console.log).catch((e) => console.error(e));\n",
    },
    Sample {
        name: "Python",
        file: "primes.py",
        src: "from math import isqrt\n\ndef primes(limit: int) -> list[int]:\n    \"\"\"Sieve of Eratosthenes up to `limit`.\"\"\"\n    sieve = [True] * (limit + 1)\n    for n in range(2, isqrt(limit) + 1):\n        if sieve[n]:\n            for m in range(n * n, limit + 1, n):\n                sieve[m] = False\n    return [n for n in range(2, limit + 1) if sieve[n]]\n\n\nif __name__ == \"__main__\":\n    print(primes(50))\n",
    },
    Sample {
        name: "Go",
        file: "server.go",
        src: "package main\n\nimport (\n\t\"fmt\"\n\t\"net/http\"\n)\n\nfunc handler(w http.ResponseWriter, r *http.Request) {\n\tfmt.Fprintf(w, \"hello, %s\", r.URL.Path[1:])\n}\n\nfunc main() {\n\thttp.HandleFunc(\"/\", handler)\n\thttp.ListenAndServe(\":8080\", nil)\n}\n",
    },
    Sample {
        name: "C",
        file: "fib.c",
        src: "#include <stdio.h>\n\n// Iterative Fibonacci.\nlong fib(int n) {\n    long a = 0, b = 1;\n    for (int i = 0; i < n; i++) {\n        long next = a + b;\n        a = b;\n        b = next;\n    }\n    return a;\n}\n\nint main(void) {\n    printf(\"%ld\\n\", fib(20));\n    return 0;\n}\n",
    },
    Sample {
        name: "Java",
        file: "Greeter.java",
        src: "import java.util.List;\n\npublic class Greeter {\n    private final String greeting;\n\n    public Greeter(String greeting) {\n        this.greeting = greeting;\n    }\n\n    public void greetAll(List<String> names) {\n        for (String name : names) {\n            System.out.println(greeting + \", \" + name + \"!\");\n        }\n    }\n}\n",
    },
    Sample {
        name: "JSON",
        file: "package.json",
        src: "{\n  \"name\": \"pebbles-code-editor\",\n  \"version\": \"0.1.0\",\n  \"private\": true,\n  \"languages\": [\"rust\", \"typescript\", \"python\", \"go\"],\n  \"editor\": {\n    \"tabSize\": 4,\n    \"insertSpaces\": true,\n    \"lineNumbers\": true\n  }\n}\n",
    },
];
