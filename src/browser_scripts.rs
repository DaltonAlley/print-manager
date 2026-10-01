#[cfg(any(feature = "ssr", test))]
pub(crate) const UNSAVED_GUARD: &str = r"(()=>{
const marked='form[data-unsaved-warning]';
let allowUnload=false,historyIndex=Number(history.state?.__pmIndex??0),restoring=false,lastUrl=location.href;
const forms=()=>Array.from(document.querySelectorAll(marked));
const dirty=form=>form.hasAttribute('data-unsaved-dirty')||form.hasAttribute('data-unsaved-initial');
const dirtyForms=except=>forms().filter(form=>form!==except&&dirty(form));
const warning=list=>list[0]?.getAttribute('data-unsaved-warning')||'Your unsaved changes will be lost.';
const clean=form=>{form.removeAttribute('data-unsaved-dirty');form.removeAttribute('data-unsaved-initial');};
const cleanAll=()=>forms().forEach(clean);
const arm=()=>{if(history.state?.__pmGuard)return;historyIndex+=1;history.replaceState({...history.state,__pmPage:true,__pmIndex:historyIndex},'',location.href);historyIndex+=1;history.pushState({...history.state,__pmPage:false,__pmGuard:true,__pmIndex:historyIndex},'',location.href);lastUrl=location.href;};
arm();
document.addEventListener('input',event=>event.target.closest?.(marked)?.setAttribute('data-unsaved-dirty','true'),true);
document.addEventListener('submit',event=>{const form=event.target.matches?.(marked)?event.target:null;if(!form)return;const other=dirtyForms(form);if(other.length&&!confirm(warning(other))){event.preventDefault();event.stopImmediatePropagation();return;}clean(form);if(other.length)allowUnload=true;},true);
document.addEventListener('click',event=>{if(event.defaultPrevented||event.button!==0||event.metaKey||event.ctrlKey||event.shiftKey||event.altKey)return;const anchor=event.target.closest?.('a[href]');if(!anchor||anchor.target&&anchor.target!=='_self'||anchor.hasAttribute('download'))return;const url=new URL(anchor.href,location.href);if(url.origin!==location.origin)return;const pending=dirtyForms();if(pending.length&&!confirm(warning(pending))){event.preventDefault();event.stopImmediatePropagation();return;}if(pending.length)cleanAll();setTimeout(arm,0);},true);
window.addEventListener('popstate',event=>{const next=Number(event.state?.__pmIndex);if(restoring){restoring=false;historyIndex=Number.isFinite(next)?next:historyIndex;return;}if(event.state?.__pmPage&&Number.isFinite(next)){const direction=Math.sign(next-historyIndex)||-1;historyIndex=next;const pending=dirtyForms();if(pending.length&&!confirm(warning(pending))){event.stopImmediatePropagation();restoring=true;history.go(-direction);return;}if(pending.length)cleanAll();event.stopImmediatePropagation();restoring=true;history.go(direction);return;}historyIndex=Number.isFinite(next)?next:historyIndex;setTimeout(arm,0);},true);
new MutationObserver(()=>{if(location.href!==lastUrl||!history.state?.__pmGuard)setTimeout(arm,0);}).observe(document.documentElement,{childList:true,subtree:true});
window.addEventListener('beforeunload',event=>{if(allowUnload){allowUnload=false;return;}if(dirtyForms().length){event.preventDefault();event.returnValue='';}});
})();";

#[cfg(any(feature = "ssr", test))]
pub(crate) const PDF_ACTION: &str = r#"(()=>{
const selector='a[data-pdf-action]';
const statusFor=link=>document.getElementById(link.getAttribute('aria-describedby'));
const show=(status,message)=>{if(status){status.hidden=false;status.textContent=message;}};
const restore=(link,label)=>{link.removeAttribute('aria-disabled');link.removeAttribute('aria-busy');link.removeAttribute('data-pdf-busy');link.textContent=label;};
document.addEventListener('click',async event=>{
if(event.defaultPrevented||event.button!==0||event.metaKey||event.ctrlKey||event.shiftKey||event.altKey)return;
const link=event.target.closest?.(selector);if(!link)return;
const status=statusFor(link),action=link.getAttribute('data-pdf-action'),label=link.textContent;
if(link.hasAttribute('data-pdf-busy')){event.preventDefault();show(status,'That PDF is already being prepared.');return;}
let printWindow=null;
if(action==='print'){printWindow=window.open('about:blank','_blank');if(!printWindow)return;printWindow.opener=null;}
event.preventDefault();
link.setAttribute('data-pdf-busy','true');link.setAttribute('aria-disabled','true');link.setAttribute('aria-busy','true');link.textContent=action==='print'?'Preparing print PDF…':'Preparing download…';show(status,link.textContent);
try{
const response=await fetch(link.href,{credentials:'same-origin',cache:'no-store'});if(!response.ok)throw new Error('pdf request failed');
const blob=await response.blob(),objectUrl=URL.createObjectURL(blob);
if(printWindow){printWindow.addEventListener('load',()=>{if(!printWindow.closed){printWindow.focus();printWindow.print();}}, {once:true});printWindow.location.replace(objectUrl);setTimeout(()=>URL.revokeObjectURL(objectUrl),300000);show(status,'Print PDF opened in a new window.');}
else{const temporary=document.createElement('a'),match=/filename="([^"]+)"/.exec(response.headers.get('content-disposition')||'');temporary.href=objectUrl;temporary.download=match?.[1]||'document.pdf';temporary.rel='noopener';document.body.appendChild(temporary);temporary.click();temporary.remove();setTimeout(()=>URL.revokeObjectURL(objectUrl),0);show(status,'PDF download is ready.');}
}catch(_error){printWindow?.close();show(status,'Could not prepare the PDF. Your saved document is safe; use the action to retry.');}
finally{restore(link,label);}
});
})();"#;

#[cfg(any(feature = "ssr", test))]
pub(crate) const THEME_INIT: &str = r"(()=>{try{const key='print-manager-theme',stored=localStorage.getItem(key),theme=stored==='light'||stored==='dark'?stored:matchMedia('(prefers-color-scheme: dark)').matches?'dark':'light';document.documentElement.dataset.theme=theme;}catch(_error){}})();";

#[cfg(any(feature = "hydrate", test))]
const HYDRATION_COMPLETE_EVENT: &str = "print-manager:hydration-complete";

#[cfg(any(feature = "ssr", test))]
pub(crate) const THEME: &str = r"(()=>{
const root=document.documentElement,key='print-manager-theme',hydrating=root.hasAttribute('data-print-manager-hydrating'),buttons=()=>document.querySelectorAll('[data-theme-toggle]');
let hydrationComplete=!hydrating;
const sync=()=>{if(!hydrationComplete)return;const dark=root.dataset.theme==='dark';buttons().forEach(button=>{button.setAttribute('aria-checked',String(dark));button.setAttribute('aria-label',dark?'Switch to light mode':'Switch to dark mode');button.querySelector('[data-theme-label]').textContent=dark?'Dark theme active':'Light theme active';});};
const stored=()=>{try{const value=localStorage.getItem(key);return value==='light'||value==='dark'?value:null;}catch(_error){return null;}};
document.addEventListener('click',event=>{const button=event.target.closest?.('[data-theme-toggle]');if(!button)return;const next=root.dataset.theme==='dark'?'light':'dark';root.dataset.theme=next;try{localStorage.setItem(key,next);}catch(_error){}sync();});
const media=matchMedia('(prefers-color-scheme: dark)');media.addEventListener?.('change',event=>{if(!stored()){root.dataset.theme=event.matches?'dark':'light';sync();}});
if(hydrating)window.addEventListener('print-manager:hydration-complete',()=>{hydrationComplete=true;root.removeAttribute('data-print-manager-hydrating');sync();},{once:true});else sync();
})();";

#[cfg(feature = "hydrate")]
pub(crate) fn emit_hydration_complete() {
    if let (Some(window), Ok(event)) = (
        web_sys::window(),
        web_sys::Event::new(HYDRATION_COMPLETE_EVENT),
    ) {
        let _ = window.dispatch_event(&event);
    }
}

#[cfg(any(feature = "ssr", test))]
pub(crate) const DETAIL_DIALOG: &str = r#"(()=>{
const liveDialog=()=>document.querySelector('[data-detail-dialog]');
const interactive='a,button,input,select,textarea,summary,details,label,form,[contenteditable]:not([contenteditable="false"]),[role="button"]';
const focusTrigger=(link,card)=>link||(card?.tabIndex>=0?card:card?.querySelector('a[data-detail-link]'));
let trigger=null,request=0,draggedCard=null,suppressDetail=null,suppressUntil=0;
const close=()=>{request++;const dialog=liveDialog();if(dialog?.open)dialog.close();};
document.addEventListener('dragstart',event=>{draggedCard=event.target.closest?.('[data-production-board] article[data-card-detail]')||null;},true);
document.addEventListener('dragend',()=>{if(draggedCard){suppressDetail=draggedCard.dataset.cardDetail;suppressUntil=Date.now()+500;}draggedCard=null;},true);
document.addEventListener('cancel',event=>{if(event.target.matches?.('[data-detail-dialog]')){event.preventDefault();close();}},true);
window.addEventListener('click',async event=>{
const dialog=liveDialog();
const closeButton=event.target.closest?.('[data-dialog-close]');
if(closeButton){close();return;}
if(dialog&&event.target===dialog){const box=dialog.getBoundingClientRect();if(event.clientX<box.left||event.clientX>box.right||event.clientY<box.top||event.clientY>box.bottom)close();return;}
if(event.button!==0||event.metaKey||event.ctrlKey||event.shiftKey||event.altKey)return;
const card=event.target.closest?.('[data-production-board] article[data-card-detail]');
if(card?.dataset.cardDetail===suppressDetail&&Date.now()<suppressUntil){event.preventDefault();event.stopImmediatePropagation();return;}
const link=event.target.closest?.('a[data-detail-link]');
if(!link&&(!card||event.target.closest?.(interactive)||window.getSelection()?.isCollapsed===false))return;
const destination=link?.href||card?.dataset.cardDetail;if(!destination)return;
if(!dialog||typeof dialog.showModal!=='function'){if(!link)location.assign(destination);return;}
event.preventDefault();event.stopImmediatePropagation();trigger=focusTrigger(link,card);const current=++request;
const content=dialog.querySelector('[data-dialog-content]');
content.innerHTML='<p role="status">Loading details…</p>';dialog.showModal();
dialog.addEventListener('close',()=>{request++;content.textContent='';trigger?.focus();trigger=null;},{once:true});
try{const response=await fetch(destination,{credentials:'same-origin',headers:{Accept:'text/html'}});if(!response.ok)throw new Error();const html=await response.text();if(current!==request||!dialog.open)return;const parsed=new DOMParser().parseFromString(html,'text/html'),selector='[data-detail-content]',detail=parsed.querySelector(selector)||Array.from(parsed.querySelectorAll('template')).map(template=>template.content.querySelector(selector)).find(Boolean);if(!detail)throw new Error();content.replaceChildren(detail);if(current===request&&dialog.open)dialog.querySelector('[data-dialog-close]').focus();}
catch(_error){if(current!==request)return;close();location.assign(destination);}
},true);
})();"#;

#[cfg(test)]
mod tests {
    use super::{
        DETAIL_DIALOG, HYDRATION_COMPLETE_EVENT, PDF_ACTION, THEME, THEME_INIT, UNSAVED_GUARD,
    };

    #[test]
    fn detail_dialog_preserves_native_links_and_complete_dialog_behavior() {
        for required in [
            "a[data-detail-link]",
            "[data-production-board] article[data-card-detail]",
            "event.target.closest?.(interactive)",
            "window.getSelection()?.isCollapsed===false",
            "suppressUntil=Date.now()+500",
            "const liveDialog=()=>document.querySelector",
            "const close=()=>{request++",
            "window.addEventListener('click'",
            "document.addEventListener('cancel'",
            "typeof dialog.showModal!=='function'",
            "await fetch(destination",
            "template.content.querySelector(selector)",
            "current!==request||!dialog.open",
            "catch(_error){if(current!==request)return",
            "location.assign(destination)",
            "event.target===dialog",
            "dialog.close()",
            "dialog.addEventListener('close'",
            "{once:true}",
            "event.stopImmediatePropagation()",
            "},true);",
            "trigger?.focus()",
            "[data-detail-content]",
        ] {
            assert!(DETAIL_DIALOG.contains(required), "missing {required}");
        }
    }

    #[test]
    fn detail_dialog_restores_focus_to_the_activated_surface_or_exact_link() {
        assert!(DETAIL_DIALOG.contains(
            "const focusTrigger=(link,card)=>link||(card?.tabIndex>=0?card:card?.querySelector('a[data-detail-link]'))"
        ));
        assert!(DETAIL_DIALOG.contains("trigger=focusTrigger(link,card)"));
        assert!(DETAIL_DIALOG.contains("trigger?.focus();trigger=null"));
        assert!(!DETAIL_DIALOG.contains("trigger=link||card"));
    }

    #[test]
    fn unsaved_guard_is_per_form_and_covers_router_and_history_navigation() {
        for required in [
            "closest?.(marked)?.setAttribute('data-unsaved-dirty'",
            "dirtyForms(form)",
            "clean(form)",
            "event.target.closest?.('a[href]')",
            "url.origin!==location.origin",
            "history.pushState",
            "window.addEventListener('popstate'",
            "history.go(-direction)",
            "history.go(direction)",
            "window.addEventListener('beforeunload'",
        ] {
            assert!(UNSAVED_GUARD.contains(required), "missing {required}");
        }
        assert!(!UNSAVED_GUARD.contains("let dirty="));
    }

    #[test]
    fn pdf_actions_are_progressively_enhanced_without_removing_links() {
        for required in [
            "a[data-pdf-action]",
            "hasAttribute('data-pdf-busy')",
            "aria-disabled",
            "aria-busy",
            "await fetch(link.href",
            "cache:'no-store'",
            "window.open('about:blank','_blank')",
            "printWindow.opener=null",
            "printWindow.addEventListener('load'",
            "printWindow.focus()",
            "printWindow.print()",
            "PDF download is ready.",
            "Your saved document is safe",
        ] {
            assert!(PDF_ACTION.contains(required), "missing {required}");
        }
        assert!(!PDF_ACTION.contains("window.open(objectUrl"));
    }

    #[test]
    fn theme_follows_system_then_persists_an_explicit_choice() {
        assert!(THEME_INIT.contains("prefers-color-scheme: dark"));
        assert!(THEME_INIT.contains("print-manager-theme"));
        assert!(THEME.contains("localStorage.setItem(key,next)"));
        assert!(THEME.contains("aria-checked"));
        assert!(THEME.contains("Switch to light mode"));
        assert!(THEME.contains("Switch to dark mode"));
        assert!(THEME.contains("media.addEventListener?.('change'"));
    }

    #[test]
    fn theme_waits_for_the_explicit_hydration_handshake_only_on_marked_pages() {
        for required in [
            "root.hasAttribute('data-print-manager-hydrating')",
            "let hydrationComplete=!hydrating",
            "if(!hydrationComplete)return",
            "window.addEventListener('print-manager:hydration-complete'",
            "root.removeAttribute('data-print-manager-hydrating')",
            "},{once:true});else sync();",
        ] {
            assert!(THEME.contains(required), "missing {required}");
        }
        assert!(THEME.contains(HYDRATION_COMPLETE_EVENT));
        assert!(!THEME.contains("requestAnimationFrame(sync)"));
        assert!(!THEME.contains("DOMContentLoaded"));
        assert!(!THEME.contains("window.addEventListener('load'"));
        assert!(!THEME.contains("setTimeout"));
    }
}
