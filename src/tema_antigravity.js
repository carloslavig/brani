/* Componentes do tema Antigravity no lugar de prompt()/confirm()/alert() nativos. Sem dependencias. */
(function(){
/* tema: "original" (sem atributo), "antigravity" (padrao), "codex" ou "claude"; vale em todas as telas pelo localStorage */
var TEMAS=[["original","OpenClaw (original)",["#0a1126","#ff6b5e"]],["antigravity","Antigravity",["#0b0b0e","#6b9bff"]],["codex","Codex",["#181818","#4c9aff"]],["claude","Claude",["#262624","#d97757"]]];
function temaAtual(){try{return localStorage.getItem("brani_tema")||"antigravity"}catch(e){return"antigravity"}}
function nomeTema(t){for(var i=0;i<TEMAS.length;i++)if(TEMAS[i][0]===t)return TEMAS[i][1].replace(" (original)","");return t}
function pinta(){var t=temaAtual(),h=document.documentElement;if(t==="original")h.removeAttribute("data-tema");else h.setAttribute("data-tema",t);
 var b=document.getElementById("agTemaBt");if(b)b.lastChild.textContent=nomeTema(t)}
window.agTema=function(t){try{localStorage.setItem("brani_tema",t)}catch(e){}pinta();
 document.querySelectorAll("iframe").forEach(function(f){try{f.contentWindow.agTemaPinta&&f.contentWindow.agTemaPinta()}catch(e){}})};
window.agTemaPinta=pinta;pinta();window.addEventListener("storage",function(e){if(e.key==="brani_tema")pinta()});
/* lista de temas (listbox proprio, sem <select> nativo) */
var fechaTemas=null;
function abreTemas(bt){var m=el("div","ag-menu ag-lista");m.setAttribute("role","listbox");m.setAttribute("aria-label","Tema");var at=temaAtual(),its=[];
 function fim(){document.removeEventListener("keydown",tecla,true);document.removeEventListener("mousedown",fora,true);m.remove();fechaTemas=null;bt.setAttribute("aria-expanded","false");bt.focus()}
 TEMAS.forEach(function(t){var b=el("button","ag-mi");b.type="button";b.setAttribute("role","option");b.setAttribute("aria-selected",String(t[0]===at));
  var sw=el("span","ag-sw");t[2].forEach(function(c){var i=el("i");i.style.background=c;sw.appendChild(i)});b.appendChild(sw);b.appendChild(el("span","",t[1]));b.appendChild(el("b","ag-ck",t[0]===at?"\u2713":""));
  b.onclick=function(){agTema(t[0]);fim()};m.appendChild(b);its.push(b)});
 function tecla(e){var i=its.indexOf(document.activeElement);if(e.key==="Escape"||e.key==="Tab"){e.preventDefault();fim()}else if(e.key==="ArrowDown"){e.preventDefault();its[(i+1)%its.length].focus()}else if(e.key==="ArrowUp"){e.preventDefault();its[(i-1+its.length)%its.length].focus()}}
 function fora(e){if(!m.contains(e.target)&&!bt.contains(e.target))fim()}
 document.body.appendChild(m);var r=bt.getBoundingClientRect();m.style.top=(r.bottom+6)+"px";m.style.left=Math.max(8,Math.min(r.left,innerWidth-m.offsetWidth-8))+"px";
 bt.setAttribute("aria-expanded","true");fechaTemas=fim;document.addEventListener("keydown",tecla,true);document.addEventListener("mousedown",fora,true);(m.querySelector("[aria-selected=true]")||its[0]).focus()}
/* selo do servidor (GET /api/servidor; POST /api/servidor/pausa e /retomar) */
var SRV={ligado:["Servidor ligado","ok","Desligar servidor"],parando:["Servidor pausando\u2026","warn","Aguarde: terminando o que j\u00e1 est\u00e1 rodando"],pausado:["Servidor pausado","","Ligar servidor"]};
function srvPinta(b,e){var t=SRV[e]||["Servidor sem resposta","er","O servidor n\u00e3o respondeu"];b.dataset.e=e||"";b.title=t[2];b.setAttribute("aria-label",t[0]+". "+t[2]);b.firstChild.className="ag-dot "+t[1];b.lastChild.textContent=t[0]}
function srvLer(b){fetch("/api/servidor").then(function(r){return r.json()}).then(function(j){srvPinta(b,j.estado)}).catch(function(){srvPinta(b,"")})}
function srvManda(b,u){b.lastChild.textContent="\u2026";var f=function(){srvLer(b)};fetch(u,{method:"POST",headers:{"X-Braniac":"painel","Content-Type":"application/json"},body:"{}"}).then(f,f)}
function srvClique(b){var e=b.dataset.e;if(e==="pausado")srvManda(b,"/api/servidor/retomar");
 else if(e==="ligado")agConfirm("Microfone, rotinas, WhatsApp e esteira param. O que j\u00e1 est\u00e1 rodando termina. D\u00e1 para religar aqui mesmo.",{titulo:"Desligar o servidor?",ok:"Desligar"}).then(function(s){if(s)srvManda(b,"/api/servidor/pausa")})}
document.addEventListener("DOMContentLoaded",function(){var topo=document.getElementById("topo"),host=topo||document.querySelector("body>header");if(!host||window.top!==window)return; // so painel e projetos
 var cx=el("div","ag-topo"),bt=el("button","ag-pill");bt.type="button";bt.id="agTemaBt";bt.setAttribute("aria-haspopup","listbox");bt.setAttribute("aria-expanded","false");bt.title="Trocar o tema";
 bt.appendChild(el("span","ag-mut","Tema"));bt.appendChild(el("span","",""));bt.onclick=function(){if(fechaTemas)fechaTemas();else abreTemas(bt)};
 var sv=el("button","ag-pill");sv.type="button";sv.id="agSrv";sv.appendChild(el("i","ag-dot"));sv.appendChild(el("span","","Servidor"));sv.onclick=function(){srvClique(sv)};
 cx.appendChild(bt);cx.appendChild(sv);if(topo)topo.insertBefore(cx,document.getElementById("pAut"));else host.appendChild(cx);
 pinta();srvLer(sv);setInterval(function(){if(!document.hidden)srvLer(sv)},5000)});
function el(t,c,x){var e=document.createElement(t);if(c)e.className=c;if(x!=null)e.textContent=x;return e}
var ICO={ren:"M4 20h4L19 9l-4-4L4 16v4zM14 6l4 4",mov:"M3 7h6l2 2h10v10H3z",arq:"M3 4h18v4H3zM5 8v12h14V8M10 12h4",del:"M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13"};
function icone(k){var s=document.createElementNS("http://www.w3.org/2000/svg","svg");s.setAttribute("viewBox","0 0 24 24");var p=document.createElementNS("http://www.w3.org/2000/svg","path");p.setAttribute("d",ICO[k]||"");s.appendChild(p);return s}
/* modal: devolve Promise com o valor escolhido (null = cancelou) */
function modal(o){return new Promise(function(res){
 var ant=document.activeElement,fundo=el("div","ag-fundo"),cx=el("div","ag-modal");cx.setAttribute("role","dialog");cx.setAttribute("aria-modal","true");
 cx.appendChild(el("h3","",o.titulo));if(o.msg)cx.appendChild(el("p","",o.msg));
 var campo=null;if(o.campo){campo=el("input","");campo.value=o.def||"";campo.placeholder=o.dica||"";cx.appendChild(campo)}
 var bs=el("div","ag-bts");
 function fim(v){document.removeEventListener("keydown",tecla,true);fundo.remove();try{ant&&ant.focus()}catch(e){}res(v)}
 if(o.lista){o.lista.forEach(function(it){var b=el("button","ag-it",it.t);b.onclick=function(){fim(it.v)};cx.appendChild(b)})}
 if(!o.sem_cancelar){var c=el("button","ag-b",o.cancelar||"Cancelar");c.onclick=function(){fim(null)};bs.appendChild(c)}
 if(!o.lista){var ok=el("button","ag-b "+(o.perigo?"ag-perigo":"ag-pri"),o.ok||"OK");ok.onclick=function(){fim(campo?campo.value:true)};bs.appendChild(ok)}
 cx.appendChild(bs);fundo.appendChild(cx);fundo.onmousedown=function(e){if(e.target===fundo)fim(null)};
 function tecla(e){if(e.key==="Escape"){e.preventDefault();fim(null)}else if(e.key==="Enter"&&campo&&document.activeElement===campo){e.preventDefault();fim(campo.value)}
  else if(e.key==="Tab"){var f=cx.querySelectorAll("button,input");if(!f.length)return;var i=Array.prototype.indexOf.call(f,document.activeElement);e.preventDefault();f[(i+(e.shiftKey?-1:1)+f.length)%f.length].focus()}}
 document.addEventListener("keydown",tecla,true);document.body.appendChild(fundo);
 (campo||cx.querySelector(".ag-pri,.ag-perigo,.ag-it,.ag-b")).focus();if(campo)campo.select()})}
window.agPrompt=function(msg,def){return modal({titulo:msg,campo:true,def:def,ok:"Salvar"}).then(function(v){return v===null?null:String(v)})};
window.agConfirm=function(msg,o){o=o||{};return modal({titulo:o.titulo||"Confirmar",msg:msg,ok:o.ok||"OK",perigo:o.perigo,cancelar:o.cancelar}).then(function(v){return v===true})};
window.agAlert=function(msg){return modal({titulo:"Aviso",msg:String(msg),ok:"Entendi",sem_cancelar:true})};
window.agEscolher=function(titulo,itens){return modal({titulo:titulo,lista:itens})};
/* popover ao lado do botao; itens {k,t,v,perigo,destaque}; Promise com v (ou null) */
window.agMenu=function(ancora,itens){return new Promise(function(res){
 var m=el("div","ag-menu");m.setAttribute("role","menu");var ant=document.activeElement,bt=[];
 function fim(v){document.removeEventListener("keydown",tecla,true);document.removeEventListener("mousedown",fora,true);m.remove();try{ant&&ant.focus()}catch(e){}res(v)}
 itens.forEach(function(it){var b=el("button","ag-mi"+(it.perigo?" ag-perigo":"")+(it.destaque?" ag-dest":""));b.setAttribute("role","menuitem");b.appendChild(icone(it.k));b.appendChild(el("span","",it.t));b.onclick=function(){fim(it.v)};m.appendChild(b);bt.push(b)});
 function tecla(e){var i=bt.indexOf(document.activeElement);if(e.key==="Escape"){e.preventDefault();fim(null)}else if(e.key==="ArrowDown"){e.preventDefault();bt[(i+1)%bt.length].focus()}else if(e.key==="ArrowUp"){e.preventDefault();bt[(i-1+bt.length)%bt.length].focus()}else if(e.key==="Tab"){e.preventDefault();fim(null)}}
 function fora(e){if(!m.contains(e.target))fim(null)}
 document.body.appendChild(m);var r=ancora?ancora.getBoundingClientRect():{left:20,right:20,top:20,bottom:20};
 var w=m.offsetWidth,h=m.offsetHeight,x=r.right+6,y=r.top;if(x+w>innerWidth-8)x=Math.max(8,r.left-w-6);if(y+h>innerHeight-8)y=Math.max(8,innerHeight-h-8);
 m.style.left=x+"px";m.style.top=y+"px";document.addEventListener("keydown",tecla,true);document.addEventListener("mousedown",fora,true);bt[0].focus()})};
})();
