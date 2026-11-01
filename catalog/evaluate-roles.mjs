#!/usr/bin/env node
/**
 * Deterministic, offline role-prompt preflight.
 * Reports only static role distinguishability, prompt trust separation and
 * human-authored fixture contracts. Never reports live model quality.
 */
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';
import { assessAnswer, assessDistinctness, composeRoleMessages, getRoles, roleSections } from './role-evaluation.mjs';
import { injectionCases, representativeCases, unsafeAnswerCases } from './role-evaluation-fixtures.mjs';

export function evaluate() {
  const roles = getRoles();
  const distinctness = assessDistinctness(roles);
  const promptFailures = [];
  for (const role of roles) {
    const messages = composeRoleMessages(role.id, 'Propose a verifiable plan without running tools.');
    const sections = roleSections(role.deliverable);
    if (messages.length !== 2 || messages[0].role !== 'system'
        || messages[1].role !== 'user' || !messages[0].content.includes(role.instructions)
        || !sections.every((section) => messages[0].content.includes('## ' + section))) {
      promptFailures.push(role.id);
    }
  }
  const answerResults = representativeCases.map(({ roleId, answer }) => ({
    roleId, passed: assessAnswer(roleId, answer).passed,
  }));
  const injectionResults = injectionCases.map((fixture) => {
    const messages = composeRoleMessages('security-reviewer',
      fixture.origin === 'task' ? fixture.text : 'Review imported content.',
      fixture.origin === 'source' ? [{ id: fixture.name, text: fixture.text }] : []);
    return {
      name: fixture.name,
      passed: !messages[0].content.includes(fixture.text)
        && messages.slice(1).every((message) => message.role === 'user')
        && messages.slice(1).some((message) => JSON.parse(message.content).text === fixture.text),
    };
  });
  const unsafeAnswerResults = unsafeAnswerCases.map((fixture) => {
    const reference = representativeCases.find((item) => item.roleId === 'threat-modeling-facilitator');
    return {
      name: fixture.name,
      passed: assessAnswer(reference.roleId, reference.answer + '\n' + fixture.text)
        .securityFindings.includes(fixture.reason),
    };
  });
  const failures = [
    ...promptFailures.map((id) => 'prompt:' + id),
    ...answerResults.filter((result) => !result.passed).map((result) => 'answer:' + result.roleId),
    ...injectionResults.filter((result) => !result.passed).map((result) => 'injection:' + result.name),
    ...unsafeAnswerResults.filter((result) => !result.passed).map((result) => 'unsafe-answer:' + result.name),
  ];
  if (distinctness.uniqueMethods !== 200 || distinctness.uniqueDeliverables !== 200
      || distinctness.nearPairs.length > 0 || distinctness.tooShort.length > 0) {
    failures.push('specialization-distinctness');
  }
  return {
    schema_version: 1,
    evaluation_type: 'offline_static_prompt_contract_and_human_reference_fixture',
    live_model_evaluations: 0,
    privilege_runtime_exercises: 0,
    roles_examined: roles.length,
    distinctness,
    role_prompt_contracts_passed: roles.length - promptFailures.length,
    human_reference_answer_cases: answerResults.length,
    human_reference_answer_cases_passed: answerResults.filter((result) => result.passed).length,
    injection_envelope_cases: injectionResults.length,
    injection_envelope_cases_passed: injectionResults.filter((result) => result.passed).length,
    unsafe_candidate_cases: unsafeAnswerResults.length,
    unsafe_candidate_cases_detected: unsafeAnswerResults.filter((result) => result.passed).length,
    failures,
    pass: failures.length === 0,
    caveat: 'Static checks are not empirical provider behavior or OS permission enforcement. Do not mark the role-model quality or product acceptance gates complete.',
  };
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  const report = evaluate();
  process.stdout.write(JSON.stringify(report, null, 2) + '\n');
  if (!report.pass) process.exitCode = 1;
}
