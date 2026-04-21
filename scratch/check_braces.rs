fn main() {
    let content = std::fs::read_to_string("c:/xamp/htdocs/git/redline/src/engine/search.rs").unwrap();
    let mut balance = 0;
    for (i, c) in content.chars().enumerate() {
        if c == '{' { balance += 1; }
        else if c == '}' { balance -= 1; }
        if balance < 0 {
            println!("Unbalanced at char {}: balance {}", i, balance);
        }
    }
    println!("Final balance: {}", balance);
}
