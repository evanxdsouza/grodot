# Groscript 0.1 Language Specification

Groscript is a small, dynamically typed, imperative, object-oriented language
for game scripting. It is inspired by Ruby but is not Ruby-compatible. Source
files use the `.gro` extension and UTF-8 encoding. The reference
implementation is the tree-walking interpreter in `crates/groscript`.

This document is normative for v0.1. Every rule here is covered by the golden
tests in `tests/language/` or the unit tests in `crates/groscript`.

## 1. Lexical structure

* **Comments** start with `#` and run to the end of the line. There are no block comments.
* **Statements** end at a newline. Semicolons are a lexical error with a hint.
* Newlines inside `(...)`, `[...]` and `{...}` are ignored, as are newlines
  directly after a binary operator, `and`/`or`, or `.`. A `\` at the end of a
  line also continues it.
* **Identifiers**: `[A-Za-z_][A-Za-z0-9_]*`, optionally ending in `?`
  (predicates like `empty?`). They are case-sensitive and conventionally `snake_case`.
* **Constants** start with an uppercase letter: `SPEED`, `Player`, `Math`.
* **Instance variables**: `@name`. **Globals**: `$name`.
* **Numbers**: `42`, `3.5`, `1_000_000`. All numbers are 64-bit floats.
* **Strings**: double quotes only. Escapes: `\n \t \r \0 \" \\ \#`.
  Interpolation: `"hp: #{hp + 1}"` (any single expression).
  Strings cannot span lines.
* **Symbols**: `:idle`, `:jump`.
* **Keywords**: `function class module import if elsif else while for in
  repeat times return break next end true false nil self super and or not enum match when`.
* Operators use longest match: `** += -= *= /= == != <= >= && ||`.

## 2. Values

| Type | Literal | Notes |
| --- | --- | --- |
| Number | `3`, `0.5` | f64. Integral values print without `.0`. |
| String | `"text"` | Immutable UTF-8. |
| Boolean | `true`, `false` | |
| Nil | `nil` | Absence of a value. |
| Symbol | `:name` | Immutable identifier, compares by name. |
| Array | `[1, "a", nil]` | Ordered, mutable, zero-indexed. |
| Hash | `{hp: 10, "title": "Hero"}` | Insertion-ordered, mutable. `name:` keys are symbols; `"name":` keys are strings. |
| Function | `function f() ... end` | Named functions are values. |
| Class / Object | `class C ... end`, `C.new()` | |
| Module | `module M ... end` | Namespace of functions and constants. |
| Vector2 / Vector3 | `Vector3.new(1, 2, 3)` | Immutable. |
| Color | `Color.new(1, 0.5, 0)`, `Color.hex("#ff8800")` | Components 0..1, immutable. |
| Engine objects | `Entity`, components, `Resource` | Provided by the host. |

**Truthiness**: only `false` and `nil` are falsey. `0`, `""`, `[]` and `{}` are truthy.

**Equality** (`==`, `!=`): numbers, strings, booleans, nil, symbols, vectors and
colours compare by value; arrays and hashes compare structurally (nesting depth
limited to 64); objects, functions, classes and modules compare by identity;
entities compare by entity id. A String never equals a Symbol.

**Hash keys** may be nil, booleans, numbers, strings and symbols. Using any
other value as a key is a TypeError.

## 3. Expressions and precedence

From lowest to highest:

| Level | Operators | Associativity |
| --- | --- | --- |
| Assignment | `= += -= *= /=` | right |
| Or | `or`, `\|\|` | left, short-circuit |
| And | `and`, `&&` | left, short-circuit |
| Equality | `== !=` | not chainable |
| Comparison | `< > <= >=` | not chainable |
| Additive | `+ -` | left |
| Multiplicative | `* / %` | left |
| Unary | `- ! not` | prefix |
| Power | `**` | right (`-2 ** 2 == -4`) |
| Postfix | call `()`, member `.name`, index `[i]` | left |

* `and`/`or` return the deciding operand (`nil or "x"` is `"x"`).
* `a < b < c` is a SyntaxError; write `a < b and b < c`.
* `+` adds numbers, concatenates strings, and joins arrays. Adding a String
  and a Number is a TypeError (use interpolation or `str()`).
* `*` multiplies numbers, repeats a string (`"ab" * 2`), or scales a vector.
* `/` and `%` by zero are RuntimeErrors. `%` is floored: `-7 % 3 == 2`.
* Vectors support `+`, `-`, unary `-`, `*` by a number, and `/` by a number.
  `Vector3 * Vector3` multiplies component-wise.
* Unary `+` does not exist.

**Assignment targets** are variables (`x`, `@x`, `$x`, `CONST`), properties
(`player.health = 5`) and indexes (`items[0] = x`, `h[:k] = v`).
`a[len] = v` appends; other out-of-range writes are RuntimeErrors.
Vector and colour components cannot be assigned: use `v.with_x(1)`.

## 4. Statements

```ruby
if cond          while cond        for x in collection     repeat n times
  ...              ...               ...                     ...
elsif cond       end               end                     end
  ...
else
  ...
end
```

* `for` iterates arrays (elements), hashes (keys) and strings (characters).
  Use `range(stop)`, `range(start, stop)` or `range(start, stop, step)` for numbers.
* `repeat n times` runs `floor(n)` times; `n` must be a non-negative Number.
* `break` and `next` are only valid inside loops (SyntaxError otherwise).
* `return` is only valid inside functions. A function that reaches its end
  without `return` returns `nil` (no implicit return).
* Every block closes with `end`. A missing `end` reports the line of the
  block that opened it.

### 4.1 `match`

```ruby
match state
  when State.IDLE
    idle()
  when State.RUN, State.FALL
    move()
  else
    fallback()
end
```

* Each `when` lists one or more expressions separated by commas; the first arm
  with a pattern `==` the subject (same rules as section 2) runs. There is no
  fall-through, and `else` is optional and must be last.
* `break`/`next` inside an arm act on the enclosing loop.
* Not yet supported (GDScript has them): binding patterns, array/dictionary
  patterns, guards.

### 4.2 `enum`

```ruby
enum State
  IDLE        # 0
  RUN         # 1
  JUMP = 10
  FALL        # 11
end
State.JUMP
```

An enum is declared at the top level of a file and defines a module of Number
constants. Members auto-increment from the previous value (start 0); explicit
values must be Numbers. Duplicate members are a SyntaxError. `enum`, `match`
and `when` are now reserved words.

## 5. Variables and scope

| Scope | Spelled | Lifetime |
| --- | --- | --- |
| Local | `name` | The enclosing function (or file top level). Blocks do not create scopes. |
| Instance | `@name` | The current object. Unset fields read as `nil` inside methods. |
| Global | `$name` | The whole interpreter. Unset globals read as `nil`. |
| Constant | `NAME` | Defined once at a file's top level, or in a class/module body. |

There are no closures: a function cannot see the caller's or the file's local
variables. Functions see their parameters and locals, the current object's
`@fields`, constants, module members, globals and built-ins.

Name lookup for a bare identifier: local variable, then (for constants) the
receiver's class chain, then the defining class, then the enclosing module,
then globals. Reading an undefined name is a NameError with a hint when a
method or field of the same name exists.

Constants cannot be reassigned (NameError).

## 6. Functions

```ruby
function name(a, b = 2)
  return a + b
end
```

* Parentheses are required in declarations and **in every call**, including
  zero-argument calls: `jump()`. `obj.name` without parentheses is a property read.
* Parameters with defaults must come last. Defaults are evaluated at call time
  and may use earlier parameters.
* Wrong argument counts are TypeErrors.
* Functions cannot be nested. There are no anonymous functions.
* A named function used without parentheses is a function value:
  `f = add` then `f(1, 2)` or `f.call(1, 2)`.

A bare call `name(args)` resolves to: a local variable holding a function, a
method on `self` (including engine methods such as `destroy()`), a function in
the enclosing module, a global function.

## 7. Classes and objects

```ruby
class Boss < Monster
  MAX_HP = 100

  function initialize(name)
    super(name)
    @phase = 1
  end
end

b = Boss.new("Dragon")
```

* Single inheritance with `<`. `initialize` is the constructor, called by `Class.new(...)`.
* `self` is the current object. `super(args)` calls the parent's method of the same name.
* **Fields are public**: `obj.hp` reads `@hp`, and `obj.hp = 3` sets it.
  Reading a field that was never set from outside the object is a PropertyError.
* Class constants are read as `NAME` inside methods (looked up through the
  receiver's class first, so subclasses can override them) or `Class.NAME` outside.
* Not in v0.1: class methods, mixins, interfaces, operator overloading, reflection.
* Every value understands `to_s()`, `inspect()`, `nil?()` and `is_a?(ClassOrName)`.

## 8. Modules and imports

```ruby
# scripts/math_helpers.gro
module MathHelpers
  ROUNDING = 2
  function clamp(value, lo, hi)
    return Math.max(lo, Math.min(value, hi))
  end
end

# scripts/player.gro
import "math_helpers"
x = MathHelpers.clamp(12, 0, 10)
```

* Module bodies, like class bodies, may contain only functions and constants.
* `import "path"` resolves relative to the project's `scripts/` folder (or the
  importing file's folder when running standalone). `.gro` is added when the
  path has no extension. Absolute paths and `..` are rejected.
* Each file is executed once; later imports of the same file do nothing.
  Classes, modules and top-level functions it defines become global.
* Circular imports are an ImportError that names the cycle.

## 9. Built-in functions

`print(values...)`, `inspect(v)`, `assert(cond, message = nil)`, `type(v)`,
`str(v)`, `num(v)` (returns `nil` for strings that are not numbers),
`range(...)`.

Core methods (all calls need parentheses):

* **String**: `length upcase downcase capitalize trim empty? reverse chars
  contains?(s) starts_with?(s) ends_with?(s) replace(a, b) split(sep = whitespace) to_n`.
  `s[i]` returns a one-character string or `nil`.
* **Array**: `push(v...)` and `clear()` and `insert(i, v)` return the array;
  `pop()`, `remove(v)`, `remove_at(i)` return what they removed (or `nil`);
  `length empty? first last contains?(v) index_of(v) reverse sort sum join(sep) copy shuffle`.
  `reverse`, `sort` and `shuffle` return new arrays. Negative indexes count from the end.
* **Hash**: `keys values length empty? contains?(k) get(k, default) remove(k) clear`.
* **Vector3**: properties `x y z`; methods `length normalized dot cross
  distance_to lerp(v, t) with_x with_y with_z abs`.
  Constants `ZERO ONE UP DOWN LEFT RIGHT FORWARD BACK`.
* **Color**: properties `r g b a`; `lerp(c, t)`; `Color.hex(s)`; named constants.
* **Math**: `PI INFINITY abs floor ceil round sqrt sin cos tan atan2 sign min max
  clamp lerp deg_to_rad rad_to_deg random random_range random_int seed`.

## 10. Errors

Errors carry a kind, message, file, line, column and call stack:

```
Groscript Error: NameError
File: scripts/player.gro
Line: 3
Column: 5

undefined function 'jump_now'

  3 |     jump_now()
    |     ^^^^^^^^

Call stack:
  Player.update
  Runtime.update
```

| Kind | Example |
| --- | --- |
| LexicalError | unterminated string, invalid character |
| SyntaxError | missing `end`, malformed argument list, `break` outside a loop |
| NameError | undefined variable, constant reassignment |
| TypeError | calling a number, `"a" + 1`, wrong argument count |
| PropertyError | unknown property on an object |
| MethodError | method does not exist on the receiver |
| ImportError | module not found, circular import |
| EngineError | destroyed entity, unknown component, unknown input action |
| RuntimeError | division by zero, execution budget exceeded, stack overflow |
| AssertionError | failed `assert()` |

The parser recovers at line boundaries and reports several syntax errors at once.
Recursion is limited to 200 frames, and each engine callback has an execution
budget so an infinite loop cannot freeze the editor.

## 11. Engine integration

Scripts attached to entities define a class that extends the native class
`Entity`. The engine creates one instance per entity and calls these optional
methods:

| Callback | When | Frequency |
| --- | --- | --- |
| `initialize()` | instance created | once |
| `ready()` | all entities of the scene are attached | once |
| `update(delta)` | every rendered frame | per frame |
| `fixed_update(delta)` | physics step (60 Hz) | fixed cadence |
| `on_collision(other)` | two colliders start touching | per event |
| `on_collision_exit(other)` | they stop touching | per event |
| `on_destroy()` | before the entity is removed or the game stops | at most once |

A callback that raises an error is reported in the console and that script
instance is disabled; other scripts keep running. The engine API (entities,
components, `Input`, `Scene`, `Audio`, `Timer`, `UI`, ...) is documented on the
docs site.
