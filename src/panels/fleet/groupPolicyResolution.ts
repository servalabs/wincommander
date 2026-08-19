export type GroupPolicyValue = string | number | boolean;

export interface GroupPolicyAssignment {
  groupId: string;
  value: GroupPolicyValue;
}

export type GroupPolicyStrategy =
  | { mode: "ranked"; order: GroupPolicyValue[] }
  | { mode: "secure-value"; secureValue: GroupPolicyValue }
  | { mode: "priority"; groupOrder: string[] }
  | { mode: "manual" };

export interface GroupPolicyResult {
  status: "resolved" | "unset" | "conflict" | "invalid";
  value?: GroupPolicyValue;
  sourceGroupIds: string[];
  hadDifferentValues: boolean;
}

function sameValue(left: GroupPolicyValue, right: GroupPolicyValue) {
  return typeof left === typeof right && left === right;
}

/** Resolves only assignments from groups the user belongs to. */
export function resolveGroupPolicy(
  memberships: string[],
  assignments: GroupPolicyAssignment[],
  strategy: GroupPolicyStrategy,
): GroupPolicyResult {
  const membershipSet = new Set(memberships);
  const applicable = assignments.filter(assignment => membershipSet.has(assignment.groupId));
  if (applicable.length === 0) {
    return { status: "unset", sourceGroupIds: [], hadDifferentValues: false };
  }

  const assignmentGroups = new Set<string>();
  for (const assignment of applicable) {
    if (assignmentGroups.has(assignment.groupId)) {
      return {
        status: "invalid",
        sourceGroupIds: applicable.map(item => item.groupId),
        hadDifferentValues: true,
      };
    }
    assignmentGroups.add(assignment.groupId);
  }

  if (strategy.mode === "ranked" && applicable.some(item =>
    !strategy.order.some(value => sameValue(value, item.value)))) {
    return {
      status: "invalid",
      sourceGroupIds: applicable.map(item => item.groupId),
      hadDifferentValues: applicable.some(item => !sameValue(item.value, applicable[0].value)),
    };
  }

  const distinct = applicable.filter((assignment, index) =>
    applicable.findIndex(candidate => sameValue(candidate.value, assignment.value)) === index);
  const hadDifferentValues = distinct.length > 1;
  if (!hadDifferentValues) {
    return {
      status: "resolved",
      value: applicable[0].value,
      sourceGroupIds: applicable.map(item => item.groupId),
      hadDifferentValues: false,
    };
  }

  if (strategy.mode === "manual") {
    return {
      status: "conflict",
      sourceGroupIds: applicable.map(item => item.groupId),
      hadDifferentValues: true,
    };
  }

  if (strategy.mode === "secure-value") {
    const secure = applicable.filter(item => sameValue(item.value, strategy.secureValue));
    if (secure.length === 0) {
      return {
        status: "conflict",
        sourceGroupIds: applicable.map(item => item.groupId),
        hadDifferentValues: true,
      };
    }
    return {
      status: "resolved",
      value: strategy.secureValue,
      sourceGroupIds: secure.map(item => item.groupId),
      hadDifferentValues: true,
    };
  }

  if (strategy.mode === "priority") {
    const prioritized = applicable
      .map(item => ({ item, index: strategy.groupOrder.indexOf(item.groupId) }))
      .filter(entry => entry.index >= 0)
      .sort((left, right) => left.index - right.index);
    if (prioritized.length === 0) {
      return {
        status: "conflict",
        sourceGroupIds: applicable.map(item => item.groupId),
        hadDifferentValues: true,
      };
    }
    return {
      status: "resolved",
      value: prioritized[0].item.value,
      sourceGroupIds: [prioritized[0].item.groupId],
      hadDifferentValues: true,
    };
  }

  const ranked = applicable.map(item => ({ item, rank: strategy.order.findIndex(value => sameValue(value, item.value)) }));
  const highestRank = Math.max(...ranked.map(entry => entry.rank));
  const winners = ranked.filter(entry => entry.rank === highestRank).map(entry => entry.item);
  return {
    status: "resolved",
    value: winners[0].value,
    sourceGroupIds: winners.map(item => item.groupId),
    hadDifferentValues: true,
  };
}
