//! The formatter

use crate::syntax::parser::{IdentMap, format::format_module, scan_identities};

fn fmt(src: &str) -> String {
    let mut idents = IdentMap::new();
    idents.insert("m".to_string(), scan_identities(src));
    match format_module(src, "m", &idents) {
        Ok(t) => t,
        Err(e) => panic!("{e:?}"),
    }
}

#[test]
fn indentation_spacing_and_blank_lines_are_normalized() {
    let src = "save var  n:int=0\n\n\nlabel start{\n\n  n+=1\n\tif n<3{\n  jump   start\n   }else{\n end\n}\n\n}\n";
    let want = "save var n: int = 0\n\nlabel start {\n    n += 1\n    if n < 3 {\n        jump start\n    } else {\n        end\n    }\n}\n";
    assert_eq!(fmt(src), want);
    assert_eq!(fmt(want), want);
}

#[test]
fn every_kind_of_line_is_written_in_the_standard_form() {
    let src = r#"import a::{b,c}
import   x as y
identity 案内人 = "案内人"
struct Item { id:str,count:int }
struct Big {
  id: str,   // the id
  // note
  count_long:  int,
  tags:list<str>
}
enum Mood { Happy, Sad(str,int) }
save var items: list<str> = ["a" ,"b"]
label start(n:int=1,m:str)->int{
  let x:int=n+(1*2) // trailing   
  案内人「こんにちは」   #a1
  案内人「二行
    目です   
    ここまで」 #a2
  おはよう #a3
  @flow.wait( 1.5 )   #pause
  let r = @input.text("名前","",8)   #name
  items.push( "c" )
  xs[0].f = 3
  call other(1,2)
  match x {
    1=>{ jump a }
    2 => end,
    _ => {
      jump b
    },
  }
  choice {
    「はい」 #yes => { jump accept ; end }
    「いいえ、断る」 if brave #no => { jump refuse }
    「ああ」 once #oo => jump x
  }
  items.each { it =>
    items.push(it)
  }
  for i , v in items{
    end
  }
  while n<3 {
    n+=1
  }
}
"#;
    let want = r#"import a::{b, c}
import x as y
identity 案内人 = "案内人"
struct Item { id: str, count: int }
struct Big {
    id:         str,  // the id
    // note
    count_long: int,
    tags:       list<str>,
}
enum Mood { Happy, Sad(str, int) }
save var items: list<str> = ["a", "b"]
label start(n: int = 1, m: str) -> int {
    let x: int = n + 1 * 2  // trailing
    案内人「こんにちは」 #a1
    案内人「二行
        目です   
        ここまで」 #a2
    おはよう #a3
    @flow.wait(1.5) #pause
    let r = @input.text("名前", "", 8) #name
    items.push("c")
    xs[0].f = 3
    call other(1, 2)
    match x {
        1 => { jump a }
        2 => end
        _ => {
            jump b
        }
    }
    choice {
        「はい」 #yes                 => { jump accept; end }
        「いいえ、断る」 if brave #no => { jump refuse }
        「ああ」 once #oo             => jump x
    }
    items.each { it =>
        items.push(it)
    }
    for i, v in items {
        end
    }
    while n < 3 {
        n += 1
    }
}
"#;
    assert_eq!(fmt(src), want);
    assert_eq!(fmt(want), want);
}

#[test]
fn multi_line_expressions_are_laid_out_one_element_per_line() {
    let src = "struct P { x: int, y: int }\nlabel start {\n    let p = P {x:1,\n  y:  2 }   // pos\n    let xs = [1,\n 2, // two\n    3\n    ]\n    let m = map { \"a\":1,\n \"bb\": 2 }\n    @log(\n  \"a\", foo(1,\n 2))\n}\n";
    let want = "struct P { x: int, y: int }\nlabel start {\n    let p = P {\n        x: 1,\n        y: 2,  // pos\n    }\n    let xs = [\n        1,\n        2,  // two\n        3,\n    ]\n    let m = map {\n        \"a\":  1,\n        \"bb\": 2,\n    }\n    @log(\n        \"a\",\n        foo(\n            1,\n            2,\n        ),\n    )\n}\n";
    assert_eq!(fmt(src), want);
    assert_eq!(fmt(want), want);
}

#[test]
fn format_source_keeps_bom_and_crlf() {
    let src = "\u{feff}label start {\r\n  end\r\n}\r\n";
    let got = crate::format_source(&[("m", src)], "m").unwrap();
    assert_eq!(got, "\u{feff}label start {\r\n    end\r\n}\r\n");
}

#[test]
fn format_source_reports_syntax_errors() {
    let src = "label start {\n    let x = (1 +\n}\n";
    assert!(matches!(
        crate::format_source(&[("m", src)], "m"),
        Err(crate::FormatError::Syntax(_))
    ));
}

#[test]
fn format_source_formats_the_migrations_module() {
    let src = "min_save_version   2\nmigrate 2 {\n  rename   v1::main.old->main.new\n convert v1::main.a:int->main.b:float = |x|to_float(x) // c\n   drop  v1::main.z\n convert v1::main.q:int->main.r:int =\n   |n| n+1 // d\n}\n";
    let want = "min_save_version 2\nmigrate 2 {\n    rename v1::main.old -> main.new\n    convert v1::main.a: int -> main.b: float = |x| to_float(x)  // c\n    drop v1::main.z\n    // d\n    convert v1::main.q: int -> main.r: int =\n        |n| n + 1\n}\n";
    let got = crate::format_source(&[("migrations", src)], "migrations").unwrap();
    assert_eq!(got, want);
    assert_eq!(
        crate::format_source(&[("migrations", want)], "migrations").unwrap(),
        want
    );
}

#[test]
fn comments_in_multi_line_expressions_are_kept() {
    let src = "label start {\n    let xs = [ // open\n        // before one\n        1,\n        2, // two\n    ]\n}\n";
    let got = fmt(src);
    assert!(got.contains("// open") && got.contains("// before one") && got.contains("// two"));
    assert_eq!(fmt(&got), got);
}
