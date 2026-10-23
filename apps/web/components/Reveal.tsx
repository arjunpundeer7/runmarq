/**
 * Section entrances without a framework. Content is fully visible without
 * script; the inline script only opts in by adding a `js` class, and an
 * IntersectionObserver reveals each `.reveal` block once as it enters. It also
 * marks the page as scrolled so the header can take on its paper backing.
 */
const SCRIPT = `(function(){
  var d=document.documentElement; d.classList.add('js');
  function sc(){d.toggleAttribute('data-scrolled',window.scrollY>8)}
  addEventListener('scroll',sc,{passive:true}); sc();
  function go(){
    var els=[].slice.call(document.querySelectorAll('.reveal:not(.is-in)'));
    if(!('IntersectionObserver' in window)){els.forEach(function(e){e.classList.add('is-in')});return;}
    var io=new IntersectionObserver(function(es){es.forEach(function(e){if(e.isIntersecting){e.target.classList.add('is-in');io.unobserve(e.target);}})},{rootMargin:'0px 0px -8% 0px'});
    els.forEach(function(e){io.observe(e)});
  }
  if(document.readyState==='loading'){document.addEventListener('DOMContentLoaded',go)}else{go()}
  new MutationObserver(function(){go()}).observe(document.documentElement,{childList:true,subtree:true});
  setTimeout(function(){[].forEach.call(document.querySelectorAll('.reveal'),function(e){e.classList.add('is-in')})},4000);
})();`;

export function RevealScript() {
  return <script dangerouslySetInnerHTML={{ __html: SCRIPT }} />;
}
