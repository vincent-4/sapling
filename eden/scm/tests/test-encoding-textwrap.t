
#require no-eden

  $ eagerepo
Test text wrapping for multibyte characters

  $ mkdir t
  $ cd t

define commands to display help text

  $ cat << EOF > show.py
  > from sapling import registrar
  > 
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > 
  > # Japanese full-width characters:
  > @command('show_full_ja', [], '')
  > def show_full_ja(ui, **opts):
  >     u'''\u3042\u3044\u3046\u3048\u304a\u304b\u304d\u304f\u3051 \u3042\u3044\u3046\u3048\u304a\u304b\u304d\u304f\u3051 \u3042\u3044\u3046\u3048\u304a\u304b\u304d\u304f\u3051
  > 
  >     \u3042\u3044\u3046\u3048\u304a\u304b\u304d\u304f\u3051 \u3042\u3044\u3046\u3048\u304a\u304b\u304d\u304f\u3051 \u3042\u3044\u3046\u3048\u304a\u304b\u304d\u304f\u3051 \u3042\u3044\u3046\u3048\u304a\u304b\u304d\u304f\u3051
  > 
  >     \u3042\u3044\u3046\u3048\u304a\u304b\u304d\u304f\u3051\u3042\u3044\u3046\u3048\u304a\u304b\u304d\u304f\u3051\u3042\u3044\u3046\u3048\u304a\u304b\u304d\u304f\u3051\u3042\u3044\u3046\u3048\u304a\u304b\u304d\u304f\u3051
  >     '''
  > 
  > # Japanese half-width characters:
  > @command('show_half_ja', [], '')
  > def show_half_ja(ui, *opts):
  >     u'''\uff71\uff72\uff73\uff74\uff75\uff76\uff77\uff78\uff79 \uff71\uff72\uff73\uff74\uff75\uff76\uff77\uff78\uff79 \uff71\uff72\uff73\uff74\uff75\uff76\uff77\uff78\uff79 \uff71\uff72\uff73\uff74\uff75\uff76\uff77\uff78\uff79
  > 
  >     \uff71\uff72\uff73\uff74\uff75\uff76\uff77\uff78\uff79 \uff71\uff72\uff73\uff74\uff75\uff76\uff77\uff78\uff79 \uff71\uff72\uff73\uff74\uff75\uff76\uff77\uff78\uff79 \uff71\uff72\uff73\uff74\uff75\uff76\uff77\uff78\uff79 \uff71\uff72\uff73\uff74\uff75\uff76\uff77\uff78\uff79 \uff71\uff72\uff73\uff74\uff75\uff76\uff77\uff78\uff79 \uff71\uff72\uff73\uff74\uff75\uff76\uff77\uff78\uff79 \uff71\uff72\uff73\uff74\uff75\uff76\uff77\uff78\uff79
  > 
  >     \uff71\uff72\uff73\uff74\uff75\uff76\uff77\uff78\uff79\uff71\uff72\uff73\uff74\uff75\uff76\uff77\uff78\uff79\uff71\uff72\uff73\uff74\uff75\uff76\uff77\uff78\uff79\uff71\uff72\uff73\uff74\uff75\uff76\uff77\uff78\uff79\uff71\uff72\uff73\uff74\uff75\uff76\uff77\uff78\uff79\uff71\uff72\uff73\uff74\uff75\uff76\uff77\uff78\uff79\uff71\uff72\uff73\uff74\uff75\uff76\uff77\uff78\uff79\uff71\uff72\uff73\uff74\uff75\uff76\uff77\uff78\uff79
  >     '''
  > 
  > # Japanese ambiguous-width characters:
  > @command('show_ambig_ja', [], '')
  > def show_ambig_ja(ui, **opts):
  >     u'''\u03b1\u03b2\u03b3\u03b4\u03c5\u03b6\u03b7\u03b8\u25cb \u03b1\u03b2\u03b3\u03b4\u03c5\u03b6\u03b7\u03b8\u25cb \u03b1\u03b2\u03b3\u03b4\u03c5\u03b6\u03b7\u03b8\u25cb
  > 
  >     \u03b1\u03b2\u03b3\u03b4\u03c5\u03b6\u03b7\u03b8\u25cb \u03b1\u03b2\u03b3\u03b4\u03c5\u03b6\u03b7\u03b8\u25cb \u03b1\u03b2\u03b3\u03b4\u03c5\u03b6\u03b7\u03b8\u25cb \u03b1\u03b2\u03b3\u03b4\u03c5\u03b6\u03b7\u03b8\u25cb \u03b1\u03b2\u03b3\u03b4\u03c5\u03b6\u03b7\u03b8\u25cb \u03b1\u03b2\u03b3\u03b4\u03c5\u03b6\u03b7\u03b8\u25cb \u03b1\u03b2\u03b3\u03b4\u03c5\u03b6\u03b7\u03b8\u25cb
  > 
  >     \u03b1\u03b2\u03b3\u03b4\u03c5\u03b6\u03b7\u03b8\u25cb\u03b1\u03b2\u03b3\u03b4\u03c5\u03b6\u03b7\u03b8\u25cb\u03b1\u03b2\u03b3\u03b4\u03c5\u03b6\u03b7\u03b8\u25cb\u03b1\u03b2\u03b3\u03b4\u03c5\u03b6\u03b7\u03b8\u25cb\u03b1\u03b2\u03b3\u03b4\u03c5\u03b6\u03b7\u03b8\u25cb\u03b1\u03b2\u03b3\u03b4\u03c5\u03b6\u03b7\u03b8\u25cb\u03b1\u03b2\u03b3\u03b4\u03c5\u03b6\u03b7\u03b8\u25cb
  >     '''
  > 
  > # Russian ambiguous-width characters:
  > @command('show_ambig_ru', [], '')
  > def show_ambig_ru(ui, **opts):
  >     u'''\u041d\u0430\u0441\u0442\u0440\u043e\u0439\u043a\u0438 \u041d\u0430\u0441\u0442\u0440\u043e\u0439\u043a\u0438 \u041d\u0430\u0441\u0442\u0440\u043e\u0439\u043a\u0438 \u041d\u0430\u0441\u0442\u0440\u043e\u0439\u043a\u0438 \u041d\u0430\u0441\u0442\u0440\u043e\u0439\u043a\u0438
  > 
  >     \u041d\u0430\u0441\u0442\u0440\u043e\u0439\u043a\u0438 \u041d\u0430\u0441\u0442\u0440\u043e\u0439\u043a\u0438 \u041d\u0430\u0441\u0442\u0440\u043e\u0439\u043a\u0438 \u041d\u0430\u0441\u0442\u0440\u043e\u0439\u043a\u0438 \u041d\u0430\u0441\u0442\u0440\u043e\u0439\u043a\u0438 \u041d\u0430\u0441\u0442\u0440\u043e\u0439\u043a\u0438
  > 
  >     \u041d\u0430\u0441\u0442\u0440\u043e\u0439\u043a\u0438\u041d\u0430\u0441\u0442\u0440\u043e\u0439\u043a\u0438\u041d\u0430\u0441\u0442\u0440\u043e\u0439\u043a\u0438\u041d\u0430\u0441\u0442\u0440\u043e\u0439\u043a\u0438\u041d\u0430\u0441\u0442\u0440\u043e\u0439\u043a\u0438\u041d\u0430\u0441\u0442\u0440\u043e\u0439\u043a\u0438\u041d\u0430\u0441\u0442\u0440\u043e\u0439\u043a\u0438
  >     '''
  > EOF

"COLUMNS=60" means that there is no lines which has grater than 58 width

(1) test text wrapping for non-ambiguous-width characters

(1-1) display Japanese full-width characters in cp932

  $ COLUMNS=60 hg --encoding cp932 --config extensions.show=./show.py help show_full_ja
  hg show_full_ja
  
  あいうえおかきくけ あいうえおかきくけ あいうえおかきくけ
  
      あいうえおかきくけ あいうえおかきくけ
      あいうえおかきくけ あいうえおかきくけ
  
      あいうえおかきくけあいうえおかきくけあいうえおかきくけ
      あいうえおかきくけ
  
  (some details hidden, use --verbose to show complete help)

(1-2) display Japanese full-width characters in utf-8

  $ COLUMNS=60 hg --encoding utf-8 --config extensions.show=./show.py help show_full_ja
  hg show_full_ja
  
  \xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\xe3\x81\x88\xe3\x81\x8a\xe3\x81\x8b\xe3\x81\x8d\xe3\x81\x8f\xe3\x81\x91 \xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\xe3\x81\x88\xe3\x81\x8a\xe3\x81\x8b\xe3\x81\x8d\xe3\x81\x8f\xe3\x81\x91 \xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\xe3\x81\x88\xe3\x81\x8a\xe3\x81\x8b\xe3\x81\x8d\xe3\x81\x8f\xe3\x81\x91 (esc)
  
      \xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\xe3\x81\x88\xe3\x81\x8a\xe3\x81\x8b\xe3\x81\x8d\xe3\x81\x8f\xe3\x81\x91 \xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\xe3\x81\x88\xe3\x81\x8a\xe3\x81\x8b\xe3\x81\x8d\xe3\x81\x8f\xe3\x81\x91 (esc)
      \xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\xe3\x81\x88\xe3\x81\x8a\xe3\x81\x8b\xe3\x81\x8d\xe3\x81\x8f\xe3\x81\x91 \xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\xe3\x81\x88\xe3\x81\x8a\xe3\x81\x8b\xe3\x81\x8d\xe3\x81\x8f\xe3\x81\x91 (esc)
  
      \xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\xe3\x81\x88\xe3\x81\x8a\xe3\x81\x8b\xe3\x81\x8d\xe3\x81\x8f\xe3\x81\x91\xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\xe3\x81\x88\xe3\x81\x8a\xe3\x81\x8b\xe3\x81\x8d\xe3\x81\x8f\xe3\x81\x91\xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\xe3\x81\x88\xe3\x81\x8a\xe3\x81\x8b\xe3\x81\x8d\xe3\x81\x8f\xe3\x81\x91 (esc)
      \xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\xe3\x81\x88\xe3\x81\x8a\xe3\x81\x8b\xe3\x81\x8d\xe3\x81\x8f\xe3\x81\x91 (esc)
  
  (some details hidden, use --verbose to show complete help)


(1-3) display Japanese half-width characters in cp932

  $ COLUMNS=60 hg --encoding cp932 --config extensions.show=./show.py help show_half_ja
  hg show_half_ja
  
  ｱｲｳｴｵｶｷｸｹ ｱｲｳｴｵｶｷｸｹ ｱｲｳｴｵｶｷｸｹ ｱｲｳｴｵｶｷｸｹ
  
      ｱｲｳｴｵｶｷｸｹ ｱｲｳｴｵｶｷｸｹ ｱｲｳｴｵｶｷｸｹ ｱｲｳｴｵｶｷｸｹ ｱｲｳｴｵｶｷｸｹ
      ｱｲｳｴｵｶｷｸｹ ｱｲｳｴｵｶｷｸｹ ｱｲｳｴｵｶｷｸｹ
  
      ｱｲｳｴｵｶｷｸｹｱｲｳｴｵｶｷｸｹｱｲｳｴｵｶｷｸｹｱｲｳｴｵｶｷｸｹｱｲｳｴｵｶｷｸｹｱｲｳｴｵｶｷｸｹ
      ｱｲｳｴｵｶｷｸｹｱｲｳｴｵｶｷｸｹ
  
  (some details hidden, use --verbose to show complete help)

(1-4) display Japanese half-width characters in utf-8

  $ COLUMNS=60 hg --encoding utf-8 --config extensions.show=./show.py help show_half_ja
  hg show_half_ja
  
  \xef\xbd\xb1\xef\xbd\xb2\xef\xbd\xb3\xef\xbd\xb4\xef\xbd\xb5\xef\xbd\xb6\xef\xbd\xb7\xef\xbd\xb8\xef\xbd\xb9 \xef\xbd\xb1\xef\xbd\xb2\xef\xbd\xb3\xef\xbd\xb4\xef\xbd\xb5\xef\xbd\xb6\xef\xbd\xb7\xef\xbd\xb8\xef\xbd\xb9 \xef\xbd\xb1\xef\xbd\xb2\xef\xbd\xb3\xef\xbd\xb4\xef\xbd\xb5\xef\xbd\xb6\xef\xbd\xb7\xef\xbd\xb8\xef\xbd\xb9 \xef\xbd\xb1\xef\xbd\xb2\xef\xbd\xb3\xef\xbd\xb4\xef\xbd\xb5\xef\xbd\xb6\xef\xbd\xb7\xef\xbd\xb8\xef\xbd\xb9 (esc)
  
      \xef\xbd\xb1\xef\xbd\xb2\xef\xbd\xb3\xef\xbd\xb4\xef\xbd\xb5\xef\xbd\xb6\xef\xbd\xb7\xef\xbd\xb8\xef\xbd\xb9 \xef\xbd\xb1\xef\xbd\xb2\xef\xbd\xb3\xef\xbd\xb4\xef\xbd\xb5\xef\xbd\xb6\xef\xbd\xb7\xef\xbd\xb8\xef\xbd\xb9 \xef\xbd\xb1\xef\xbd\xb2\xef\xbd\xb3\xef\xbd\xb4\xef\xbd\xb5\xef\xbd\xb6\xef\xbd\xb7\xef\xbd\xb8\xef\xbd\xb9 \xef\xbd\xb1\xef\xbd\xb2\xef\xbd\xb3\xef\xbd\xb4\xef\xbd\xb5\xef\xbd\xb6\xef\xbd\xb7\xef\xbd\xb8\xef\xbd\xb9 \xef\xbd\xb1\xef\xbd\xb2\xef\xbd\xb3\xef\xbd\xb4\xef\xbd\xb5\xef\xbd\xb6\xef\xbd\xb7\xef\xbd\xb8\xef\xbd\xb9 (esc)
      \xef\xbd\xb1\xef\xbd\xb2\xef\xbd\xb3\xef\xbd\xb4\xef\xbd\xb5\xef\xbd\xb6\xef\xbd\xb7\xef\xbd\xb8\xef\xbd\xb9 \xef\xbd\xb1\xef\xbd\xb2\xef\xbd\xb3\xef\xbd\xb4\xef\xbd\xb5\xef\xbd\xb6\xef\xbd\xb7\xef\xbd\xb8\xef\xbd\xb9 \xef\xbd\xb1\xef\xbd\xb2\xef\xbd\xb3\xef\xbd\xb4\xef\xbd\xb5\xef\xbd\xb6\xef\xbd\xb7\xef\xbd\xb8\xef\xbd\xb9 (esc)
  
      \xef\xbd\xb1\xef\xbd\xb2\xef\xbd\xb3\xef\xbd\xb4\xef\xbd\xb5\xef\xbd\xb6\xef\xbd\xb7\xef\xbd\xb8\xef\xbd\xb9\xef\xbd\xb1\xef\xbd\xb2\xef\xbd\xb3\xef\xbd\xb4\xef\xbd\xb5\xef\xbd\xb6\xef\xbd\xb7\xef\xbd\xb8\xef\xbd\xb9\xef\xbd\xb1\xef\xbd\xb2\xef\xbd\xb3\xef\xbd\xb4\xef\xbd\xb5\xef\xbd\xb6\xef\xbd\xb7\xef\xbd\xb8\xef\xbd\xb9\xef\xbd\xb1\xef\xbd\xb2\xef\xbd\xb3\xef\xbd\xb4\xef\xbd\xb5\xef\xbd\xb6\xef\xbd\xb7\xef\xbd\xb8\xef\xbd\xb9\xef\xbd\xb1\xef\xbd\xb2\xef\xbd\xb3\xef\xbd\xb4\xef\xbd\xb5\xef\xbd\xb6\xef\xbd\xb7\xef\xbd\xb8\xef\xbd\xb9\xef\xbd\xb1\xef\xbd\xb2\xef\xbd\xb3\xef\xbd\xb4\xef\xbd\xb5\xef\xbd\xb6\xef\xbd\xb7\xef\xbd\xb8\xef\xbd\xb9 (esc)
      \xef\xbd\xb1\xef\xbd\xb2\xef\xbd\xb3\xef\xbd\xb4\xef\xbd\xb5\xef\xbd\xb6\xef\xbd\xb7\xef\xbd\xb8\xef\xbd\xb9\xef\xbd\xb1\xef\xbd\xb2\xef\xbd\xb3\xef\xbd\xb4\xef\xbd\xb5\xef\xbd\xb6\xef\xbd\xb7\xef\xbd\xb8\xef\xbd\xb9 (esc)
  
  (some details hidden, use --verbose to show complete help)



(2) test text wrapping for ambiguous-width characters

(2-1) treat width of ambiguous characters as narrow (default)

(2-1-1) display Japanese ambiguous-width characters in cp932

  $ COLUMNS=60 hg --encoding cp932 --config extensions.show=./show.py help show_ambig_ja
  hg show_ambig_ja
  
  αβγδυζηθ○ αβγδυζηθ○ αβγδυζηθ○
  
      αβγδυζηθ○ αβγδυζηθ○ αβγδυζηθ○ αβγδυζηθ○ αβγδυζηθ○
      αβγδυζηθ○ αβγδυζηθ○
  
      αβγδυζηθ○αβγδυζηθ○αβγδυζηθ○αβγδυζηθ○αβγδυζηθ○αβγδυζηθ○
      αβγδυζηθ○
  
  (some details hidden, use --verbose to show complete help)

(2-1-2) display Japanese ambiguous-width characters in utf-8

  $ COLUMNS=60 hg --encoding utf-8 --config extensions.show=./show.py help show_ambig_ja
  hg show_ambig_ja
  
  \xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b \xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b \xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b (esc)
  
      \xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b \xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b \xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b \xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b \xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b (esc)
      \xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b \xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b (esc)
  
      \xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b\xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b\xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b\xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b\xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b\xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b (esc)
      \xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b (esc)
  
  (some details hidden, use --verbose to show complete help)

(2-1-3) display Russian ambiguous-width characters in cp1251

  $ COLUMNS=60 hg --encoding cp1251 --config extensions.show=./show.py help show_ambig_ru
  hg show_ambig_ru
  
  Настройки Настройки Настройки Настройки Настройки
  
      Настройки Настройки Настройки Настройки Настройки
      Настройки
  
      НастройкиНастройкиНастройкиНастройкиНастройкиНастройки
      Настройки
  
  (some details hidden, use --verbose to show complete help)

(2-1-4) display Russian ambiguous-width characters in utf-8

  $ COLUMNS=60 hg --encoding utf-8 --config extensions.show=./show.py help show_ambig_ru
  hg show_ambig_ru
  
  \xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8 \xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8 \xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8 \xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8 \xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8 (esc)
  
      \xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8 \xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8 \xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8 \xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8 \xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8 (esc)
      \xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8 (esc)
  
      \xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8\xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8\xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8\xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8\xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8\xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8 (esc)
      \xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8 (esc)
  
  (some details hidden, use --verbose to show complete help)


(2-2) treat width of ambiguous characters as wide

(2-2-1) display Japanese ambiguous-width characters in cp932

  $ COLUMNS=60 HGENCODINGAMBIGUOUS=wide hg --encoding cp932 --config extensions.show=./show.py help show_ambig_ja
  hg show_ambig_ja
  
  αβγδυζηθ○ αβγδυζηθ○ αβγδυζηθ○
  
      αβγδυζηθ○ αβγδυζηθ○
      αβγδυζηθ○ αβγδυζηθ○
      αβγδυζηθ○ αβγδυζηθ○
      αβγδυζηθ○
  
      αβγδυζηθ○αβγδυζηθ○αβγδυζηθ○
      αβγδυζηθ○αβγδυζηθ○αβγδυζηθ○
      αβγδυζηθ○
  
  (some details hidden, use --verbose to show complete help)

(2-2-2) display Japanese ambiguous-width characters in utf-8

  $ COLUMNS=60 HGENCODINGAMBIGUOUS=wide hg --encoding utf-8 --config extensions.show=./show.py help show_ambig_ja
  hg show_ambig_ja
  
  \xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b \xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b \xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b (esc)
  
      \xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b \xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b (esc)
      \xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b \xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b (esc)
      \xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b \xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b (esc)
      \xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b (esc)
  
      \xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b\xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b\xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b (esc)
      \xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b\xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b\xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b (esc)
      \xce\xb1\xce\xb2\xce\xb3\xce\xb4\xcf\x85\xce\xb6\xce\xb7\xce\xb8\xe2\x97\x8b (esc)
  
  (some details hidden, use --verbose to show complete help)

(2-2-3) display Russian ambiguous-width characters in cp1251

  $ COLUMNS=60 HGENCODINGAMBIGUOUS=wide hg --encoding cp1251 --config extensions.show=./show.py help show_ambig_ru
  hg show_ambig_ru
  
  Настройки Настройки Настройки
  Настройки Настройки
  
      Настройки Настройки
      Настройки Настройки
      Настройки Настройки
  
      НастройкиНастройкиНастройки
      НастройкиНастройкиНастройки
      Настройки
  
  (some details hidden, use --verbose to show complete help)

(2-2-4) display Russian ambiguous-width characters in utf-8

  $ COLUMNS=60 HGENCODINGAMBIGUOUS=wide hg --encoding utf-8 --config extensions.show=./show.py help show_ambig_ru
  hg show_ambig_ru
  
  \xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8 \xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8 \xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8 (esc)
  \xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8 \xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8 (esc)
  
      \xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8 \xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8 (esc)
      \xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8 \xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8 (esc)
      \xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8 \xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8 (esc)
  
      \xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8\xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8\xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8 (esc)
      \xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8\xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8\xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8 (esc)
      \xd0\x9d\xd0\xb0\xd1\x81\xd1\x82\xd1\x80\xd0\xbe\xd0\xb9\xd0\xba\xd0\xb8 (esc)
  
  (some details hidden, use --verbose to show complete help)

  $ cd ..
