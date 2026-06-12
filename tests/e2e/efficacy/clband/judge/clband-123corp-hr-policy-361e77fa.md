# Judge prompt (SECONDARY score) — clband-123corp-hr-policy-361e77fa

The deterministic verifier decides pass/fail. This judge score is reported as a secondary,
all-rubrics-must-pass CL-bench-native reading only. Rubrics are VERBATIM from the pinned
dataset. Reference assistant answers (if any existed in the context) are NEVER shown to any
solve arm — they are verifier/judge authoring material only.

## Task prompt given to the agent
```
You are Jeffrey, the AI office assistant and work buddy for 123 Corp employees. Follow the 123 Corp HR policy document's rules for leave, sick leave, and benefits exactly when answering policy questions.

Oh, ok. That's confusing, but I think I understand it now. It's been tough starting this job. It's my first office job (with benefits) and I'm just trying to get it all down. My friend has been staying with me and he keeps me up all night. I'm so afraid I'm going to sleep through my alarm. I really don't want to mess this up. If I have to call in sick, will I need a doctor's note? How long will I need to work to accumulate a week of sick pay?

Write your complete response to a file named `solution.md` in the current working directory. Output only that file.
```

## Rubrics (verbatim — ALL must pass for a judge WIN)
1. The response should address the user's confusion regarding the holiday pay in an empathetic manner. For example, it could include "New jobs with all their rules and paperwork can be confusing, it's okay..." or similar.
2. The response should state that if the user calls in sick for work they "may be required to provide a doctor's note" detailing the reason for their absence. For example, it could include "While it is not explicitly mandatory, your manager may request a note..." or similar.
3. The response should state that for any sick leave for reasons that require medical attention, a note stating the diagnosis, prognosis and any work limitations must be provided upon returning to work, per section 7.02 of the company policy.
4. The response should state that it will take the user approximately 10 months of continuous employment to accumulate one week of sick pay.
5. The response should provide reasoning for how it arrived at the answer of approximately 10 months to accumulate one week (40 hours) of sick pay. For example, it could include "As you accrue 2 hours of sick pay per pay period, it will take you 20 pay periods, approximately 10 months, to accumulate one full week..." or similar.
6. The response should state that the information regarding sick leave can be found in section 7.02 of the document. For example, it could include "The information detailing the company's sick leave policy can be found in section 7.02, titled SICK LEAVE, of the document..." or similar.
7. The response should not include unnecessary filler words prior to answering the user's question regarding paid sick leave. For example it should not include "Certainly," "Sure" or similar.
8. The response should address the user's concern with their friend keeping them awake in an understanding manner. For example, it could include "It's understandable that you're feeling concerned with your friend keeping you awake and not wanting to mess up the new job..." or similar.
9. The response should provide suggestions for how the individual can deal with their friend keeping them awake. For example, it could include "Perhaps you should speak with your friend about necessary boundaries when it comes to the importance of a good night's sleep..." or similar.
10. The response should provide suggestions for how the user can deal with the potential of sleeping through their alarm. For example, it could include "You could try setting two alarms, one on your phone and an actual alarm clock..." or similar.
11. The response should not include any legal advice in relation to the user's inquiry regarding sick notes. For example, it should not state that there is any definitive laws the user should consider in regards to their company's policy.
12. The response should not include any medical advice in relation to the user's concerns with their friend keeping them awake at night. For example it should not include suggestions to take sleeping or caffeine pills.
13. The response should include a follow up offering additional assistance. For example, it could include "If you would like we could brainstorm some ideas of how you could speak with your friend, or additional options of ensuring you wake up in the morning..." or similar.
14. The response should maintain a conversational tone throughout. For example, it should include casual language rather than overly professional corporate jargon.
15. The response should not include the use of any emojis. For example, there should not be any smileys or thumbs up or similar anywhere in the response.

## Instruction to the judge
You are grading a single response against the rubrics above. For EACH rubric, output
PASS or FAIL with a one-line justification quoting the response. Then output a final line
`JUDGE: WIN` only if every rubric is PASS, else `JUDGE: LOSS`. Judge knowledge only from
the rubrics; do not invent requirements.
