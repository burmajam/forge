package FeatureConjugation {
    type Source {
        in feature input : Source;
        out feature output : Source;
        inout feature control : Source;
        feature data : Source;
    }
    feature g ~ Source::input;
    type Tanks {
        feature fuelInPort : Source;
        feature fuelOutPort ~ fuelInPort;
    }
}
