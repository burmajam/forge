package Vehicles {
    type Vehicle {}
    type Car :> Vehicle {
        feature wheels : Vehicle [4];
    }
}
