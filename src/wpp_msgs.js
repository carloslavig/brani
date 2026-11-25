(function(max, EU){
  var msgs=[];
  document.querySelectorAll('[data-pre-plain-text]').forEach(function(el){
    var cab=el.getAttribute('data-pre-plain-text')||'';
    var corpo=el.querySelector('span.selectable-text, span[dir]');
    var txt=(corpo?corpo.innerText:el.innerText)||'';
    var row=el.closest('[class*="message-in"],[class*="message-out"]');
    var dir=row && row.className.indexOf('message-out')>=0 ? 'EU' : 'ELE';
    if (EU && cab.replace(/\s+/g,' ').indexOf('] '+EU+':')>=0) dir='EU';  // layout novo: so o cabecalho indica o autor
    var links=[];   // endereco dos links (17/11: o Pedro mandou um link do Drive e so o titulo da previa chegava ao Jarvis)
    (el.closest('[role="row"]')||row||el).querySelectorAll('a[href^="http"]').forEach(function(a){ if(links.indexOf(a.href)<0) links.push(a.href); });
    msgs.push({d:dir,c:cab.replace(/\s+/g,' ').trim(),t:txt.trim(),links:links});
  });
  // midia sem texto (audio/foto) aparece como linhas separadas
  var extras=[];
  var linhas=[...document.querySelectorAll('#main [role="row"], [role="application"] [role="row"]')];   // o layout novo do WhatsApp Web nao tem mais role=application
  var ultimaNossa=-1;   // indice da ultima linha enviada por NOS (texto ou arquivo): documento antes disso ja foi tratado
  // nossa = message-out, rabinho de saida ou icone de status (so mensagem nossa tem tique); audio nosso em sequencia nao tem rabinho (24/10: a mae recebeu 4 respostas)
  var nossa=function(r){ return !!(r.className.indexOf('message-out')>=0||r.querySelector('[class*="message-out"],[data-icon="tail-out"],[data-icon^="msg-check"],[data-icon^="msg-dblcheck"],[data-icon^="msg-time"],[data-icon*="status-dblcheck"],[data-icon*="status-check"]') || [...r.querySelectorAll('[aria-label]')].some(function(e){return /^\s*(Entregue|Lida|Enviada|Pendente|Delivered|Read|Sent|Pending)\s*$|^Você:$/i.test(e.getAttribute('aria-label'));})); };
  linhas.forEach(function(r,i){ if(nossa(r)) ultimaNossa=i; });
  linhas.forEach(function(r,i){
    var doc=r.querySelector('[data-icon^="document-"]');    // documento (html, pdf, docx, zip...): ha o icone document-XXX-icon e o botao 'Baixar "nome"'
    if(doc && !r.querySelector('[data-icon="tail-out"]')){
      var b=r.querySelector('[title^="Baixar"]'), m=b && (b.getAttribute('title')||'').match(/"(.+)"/);
      if(m && i>ultimaNossa) extras.push({d:'ELE',c:'',t:'[DOC:'+m[1]+']'});
      return;
    }
    if(r.querySelector('[data-pre-plain-text]')) return;
    var tipo='';
    if(r.querySelector('audio,[aria-label*="mensagem de voz" i],[data-icon*="audio" i],[data-icon*="ptt" i]')) tipo='[AUDIO]';
    else if(r.querySelector('img[src^="blob"]')) tipo='[IMAGEM]';
    if(tipo && i>ultimaNossa) extras.push({d:nossa(r)?'EU':'ELE',c:'',t:tipo});   // midia antes da nossa ultima resposta ja foi respondida
  });
  return JSON.stringify({msgs:msgs.slice(-max), midias:extras.slice(-3)});
})(@MAX@, @EU@)
