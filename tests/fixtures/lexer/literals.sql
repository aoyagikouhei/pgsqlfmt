SELECT 1_000, 0x1F, 0o17, 0b1010, .5, 5., 1.5E-3,
       'plain', E'tab\there', B'1010', X'FF', N'national', U&'d\0061t\+000061',
       U&"quoted ident", "a ""b"" c",
       $$dollar 'quoted'$$, $tag$ nested $$ $tag$,
       /* outer /* inner */ still comment */ 'after';
