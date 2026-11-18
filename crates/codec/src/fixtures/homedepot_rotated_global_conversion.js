// Synthetic full-shape excerpt of the rotated HomeDepot key layout; no executable challenge.
// Anchors match homedepot_491_split_numeric.js. Names, table indices and pool order rotate per build.
var power=["BNNN"];var anchors_a=["3vJ3JbHvJM7NNNNNN"];var anchors_b=["mrmmbNH7NNNNNN", "bMMrM7NNNNNN"];
var gAlias,cOne,cTwo,cIdx,numPool;
function tblRead(tIdx){return tblKeys()[tIdx];}
function tblKeys(){var tArr=["kA","kB","kC","kD"];tblKeys=function(){return tArr;};return tArr;}
function tblNames(){var tObj={};tblNames=function(){return tObj;};return tObj;}
function tblHelp(){var hObj={};tblHelp=function(){return hObj;};return hObj;}
function setup(state){switch(state){case 1:{cOne=+!![];cTwo=cOne+cOne;}break;case 2:{cIdx=cTwo-cOne;numPool=decode(state,[["noise","JbBmBvJ"],[]]);}break;}}
if(typeof window!=='undefined'){gAlias=window;}else if(typeof global!=='undefined'){gAlias=global;}else{gAlias=this;}
function body(session){var useKey=function(){var keyPair=[session,keyValue];return keyPair;};var keyValue=gAlias[tblNames()[tblRead(cOne)](cTwo,cOne)](tblHelp()[tblRead(cTwo)].call(null,cOne)[cmpSame(typeof tblNames()[tblRead(cIdx)],'undefined')?tblHelp()[tblRead(cOne)](cIdx):tblHelp()[tblRead(cTwo)].apply(null,[cOne,cTwo])](numPool[cIdx]));return useKey;}
