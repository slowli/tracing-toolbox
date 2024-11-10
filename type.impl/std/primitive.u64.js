(function() {
    var type_impls = Object.fromEntries([["tracing_tunnel",[["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-FromTracedValue%3C'_%3E-for-u64\" class=\"impl\"><a class=\"src rightside\" href=\"src/tracing_tunnel/value.rs.html#248\">Source</a><a href=\"#impl-FromTracedValue%3C'_%3E-for-u64\" class=\"anchor\">§</a><h3 class=\"code-header\">impl <a class=\"trait\" href=\"tracing_tunnel/trait.FromTracedValue.html\" title=\"trait tracing_tunnel::FromTracedValue\">FromTracedValue</a>&lt;'_&gt; for <a class=\"primitive\" href=\"https://doc.rust-lang.org/nightly/std/primitive.u64.html\">u64</a></h3></section></summary><div class=\"impl-items\"><details class=\"toggle\" open><summary><section id=\"associatedtype.Output\" class=\"associatedtype trait-impl\"><a class=\"src rightside\" href=\"src/tracing_tunnel/value.rs.html#248\">Source</a><a href=\"#associatedtype.Output\" class=\"anchor\">§</a><h4 class=\"code-header\">type <a href=\"tracing_tunnel/trait.FromTracedValue.html#associatedtype.Output\" class=\"associatedtype\">Output</a> = <a class=\"primitive\" href=\"https://doc.rust-lang.org/nightly/std/primitive.u64.html\">u64</a></h4></section></summary><div class='docblock'>Output of the conversion.</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.from_value\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/tracing_tunnel/value.rs.html#248\">Source</a><a href=\"#method.from_value\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"tracing_tunnel/trait.FromTracedValue.html#tymethod.from_value\" class=\"fn\">from_value</a>(value: &amp;<a class=\"enum\" href=\"tracing_tunnel/enum.TracedValue.html\" title=\"enum tracing_tunnel::TracedValue\">TracedValue</a>) -&gt; <a class=\"enum\" href=\"https://doc.rust-lang.org/nightly/core/option/enum.Option.html\" title=\"enum core::option::Option\">Option</a>&lt;Self::<a class=\"associatedtype\" href=\"tracing_tunnel/trait.FromTracedValue.html#associatedtype.Output\" title=\"type tracing_tunnel::FromTracedValue::Output\">Output</a>&gt;</h4></section></summary><div class='docblock'>Performs the conversion.</div></details></div></details>","FromTracedValue<'_>","tracing_tunnel::types::MetadataId","tracing_tunnel::types::RawSpanId"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-PartialEq%3CTracedValue%3E-for-u64\" class=\"impl\"><a class=\"src rightside\" href=\"src/tracing_tunnel/value.rs.html#248\">Source</a><a href=\"#impl-PartialEq%3CTracedValue%3E-for-u64\" class=\"anchor\">§</a><h3 class=\"code-header\">impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/cmp/trait.PartialEq.html\" title=\"trait core::cmp::PartialEq\">PartialEq</a>&lt;<a class=\"enum\" href=\"tracing_tunnel/enum.TracedValue.html\" title=\"enum tracing_tunnel::TracedValue\">TracedValue</a>&gt; for <a class=\"primitive\" href=\"https://doc.rust-lang.org/nightly/std/primitive.u64.html\">u64</a></h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.eq\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/tracing_tunnel/value.rs.html#248\">Source</a><a href=\"#method.eq\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/nightly/core/cmp/trait.PartialEq.html#tymethod.eq\" class=\"fn\">eq</a>(&amp;self, other: &amp;<a class=\"enum\" href=\"tracing_tunnel/enum.TracedValue.html\" title=\"enum tracing_tunnel::TracedValue\">TracedValue</a>) -&gt; <a class=\"primitive\" href=\"https://doc.rust-lang.org/nightly/std/primitive.bool.html\">bool</a></h4></section></summary><div class='docblock'>Tests for <code>self</code> and <code>other</code> values to be equal, and is used by <code>==</code>.</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.ne\" class=\"method trait-impl\"><span class=\"rightside\"><span class=\"since\" title=\"Stable since Rust version 1.0.0\">1.0.0</span> · <a class=\"src\" href=\"https://doc.rust-lang.org/nightly/src/core/cmp.rs.html#261\">Source</a></span><a href=\"#method.ne\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/nightly/core/cmp/trait.PartialEq.html#method.ne\" class=\"fn\">ne</a>(&amp;self, other: <a class=\"primitive\" href=\"https://doc.rust-lang.org/nightly/std/primitive.reference.html\">&amp;Rhs</a>) -&gt; <a class=\"primitive\" href=\"https://doc.rust-lang.org/nightly/std/primitive.bool.html\">bool</a></h4></section></summary><div class='docblock'>Tests for <code>!=</code>. The default implementation is almost always sufficient,\nand should not be overridden without very good reason.</div></details></div></details>","PartialEq<TracedValue>","tracing_tunnel::types::MetadataId","tracing_tunnel::types::RawSpanId"]]]]);
    if (window.register_type_impls) {
        window.register_type_impls(type_impls);
    } else {
        window.pending_type_impls = type_impls;
    }
})()
//{"start":55,"fragment_lengths":[4842]}