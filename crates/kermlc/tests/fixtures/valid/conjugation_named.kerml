package NamedConjugation {
    type Source {
        in feature input : Source;
        out feature output : Source;
        inout feature control : Source;
        feature data : Source;
    }

    type Sink {}

    conjugation c1 conjugate Sink conjugates Source;
}
